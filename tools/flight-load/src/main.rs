// SPDX-License-Identifier: MIT

//! Measurements against a running orchestrator. Not part of the product binary: it only
//! reads through the same `UiClient` a dashboard uses, and triggers state changes by running
//! a command the operator supplies.
//!
//! `flight-load lan --ui-dir DIR --node NAME --session S --perm CMD --idle CMD
//!    [--cycles N] [--previews N]`
//!
//! Reports: connect→converged, state-change latency (trigger→seen in the UI image, both
//! the time since the trigger command started and since it returned), preview round trips,
//! and the bytes the orchestrator sent this UI.

mod observe;
mod synth;

use flight_client::ClientConfig;
use flight_proto::{
    command_kind as ck, ui_event_body, ui_request_body, Command, FleetImage, PaneRefMsg, Request,
    StateCode, Step, Subscribe, UiEvent, UiRequest,
};
use flight_state::PaneRef;
use flight_transport::UiClient;
use prost::Message;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command as Shell;

struct Args {
    ui_dir: PathBuf,
    node: String,
    session: String,
    perm: String,
    idle: String,
    cycles: u32,
    previews: u32,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        ui_dir: PathBuf::new(),
        node: String::new(),
        session: String::new(),
        perm: String::new(),
        idle: String::new(),
        cycles: 10,
        previews: 20,
    };
    let mut it = std::env::args().skip(1);
    match it.next().as_deref() {
        Some("lan") => {}
        Some("watch") => return Err("watch".into()),
        Some("synth") => return Err("synth".into()),
        Some("observe") => return Err("observe".into()),
        _ => return Err("usage: flight-load watch --ui-dir DIR --secs N | lan --ui-dir DIR --node NAME --session S --perm CMD --idle CMD [--cycles N] [--previews N]".into()),
    }
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--ui-dir" => a.ui_dir = val()?.into(),
            "--node" => a.node = val()?,
            "--session" => a.session = val()?,
            "--perm" => a.perm = val()?,
            "--idle" => a.idle = val()?,
            "--cycles" => a.cycles = val()?.parse().map_err(|_| "bad --cycles")?,
            "--previews" => a.previews = val()?.parse().map_err(|_| "bad --previews")?,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if a.node.is_empty() || a.session.is_empty() || a.perm.is_empty() || a.idle.is_empty() {
        return Err("--node, --session, --perm and --idle are required".into());
    }
    Ok(a)
}

struct Session {
    client: UiClient,
    image: FleetImage,
    bytes: u64,
    events: u64,
}

impl Session {
    async fn next(&mut self) -> Result<(), String> {
        match self.client.next_event().await.map_err(|e| e.to_string())? {
            Some(event) => {
                self.bytes += event.encoded_len() as u64;
                self.events += 1;
                if self.image.apply(&event) == Step::Resync {
                    return Err("resync requested".into());
                }
                Ok(())
            }
            None => Err("stream closed".into()),
        }
    }

    /// The state of the named pane, if the node and pane are in the image.
    fn state_of(&self, node: &str, session: &str) -> Option<(PaneRef, i32)> {
        let n = self
            .image
            .nodes()
            .values()
            .find(|n| n.display_name == node)?;
        n.panes
            .iter()
            .find(|(_, p)| p.session == session)
            .map(|(r, p)| (r.clone(), p.state))
    }

    async fn wait_state(
        &mut self,
        node: &str,
        session: &str,
        want_permit: bool,
        limit: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some((_, s)) = self.state_of(node, session) {
                if (s == StateCode::Permit as i32) == want_permit {
                    return Ok(());
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("timed out waiting for the state change".into());
            }
            tokio::time::timeout(left, self.next())
                .await
                .map_err(|_| "timed out waiting for the state change".to_owned())??;
        }
    }
}

async fn run_cmd(cmd: &str) -> Result<(Instant, Instant), String> {
    let start = Instant::now();
    let status = Shell::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .await
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("trigger failed: {cmd}"));
    }
    Ok((start, Instant::now()))
}

fn stats(label: &str, mut v: Vec<f64>) {
    if v.is_empty() {
        println!("{label}: no samples");
        return;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let pick = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
    println!(
        "{label}: n={} min={:.0} p50={:.0} p90={:.0} max={:.0} (ms)",
        v.len(),
        v[0],
        pick(0.5),
        pick(0.9),
        v[v.len() - 1]
    );
}

#[tokio::main]
async fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) if e == "watch" => return watch().await,
        Err(e) if e == "observe" => {
            if let Err(e) = observe::main(std::env::args().skip(2)) {
                eprintln!("flight-load: {e}");
                std::process::exit(1);
            }
            return;
        }
        Err(e) if e == "synth" => {
            let parsed = synth::Args::parse(std::env::args().skip(2));
            match parsed {
                Ok(a) => {
                    if let Err(e) = synth::run(a).await {
                        eprintln!("flight-load: {e}");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            }
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args).await {
        eprintln!("flight-load: {e}");
        std::process::exit(1);
    }
}

async fn run(a: Args) -> Result<(), String> {
    let config = ClientConfig::load(&a.ui_dir.join("ui")).map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let client = UiClient::connect(&config.address, &config.identity, &config.orchestrator)
        .await
        .map_err(|e| e.to_string())?;
    let connected = t0.elapsed();
    client
        .send(UiRequest {
            body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
        })
        .map_err(|e| e.to_string())?;
    let mut s = Session {
        client,
        image: FleetImage::new(),
        bytes: 0,
        events: 0,
    };
    while !s.image.in_sync() {
        s.next().await?;
    }
    let converged = t0.elapsed();
    println!(
        "connect {:.0} ms, first snapshot {:.0} ms ({} bytes)",
        connected.as_secs_f64() * 1e3,
        converged.as_secs_f64() * 1e3,
        s.bytes
    );
    let (pane, _) = s
        .state_of(&a.node, &a.session)
        .ok_or("pane not found: is the node online and the session an agent?")?;

    // Start from idle so the first cycle is a real change.
    run_cmd(&a.idle).await?;
    s.wait_state(&a.node, &a.session, false, Duration::from_secs(15))
        .await?;
    let base_bytes = s.bytes;
    let t_run = Instant::now();

    let (mut since_start, mut since_end) = (Vec::new(), Vec::new());
    for want_permit in std::iter::repeat_n([true, false], a.cycles as usize).flatten() {
        let cmd = if want_permit { &a.perm } else { &a.idle };
        let (started, returned) = run_cmd(cmd).await?;
        s.wait_state(&a.node, &a.session, want_permit, Duration::from_secs(15))
            .await?;
        let seen = Instant::now();
        since_start.push((seen - started).as_secs_f64() * 1e3);
        since_end.push(seen.saturating_duration_since(returned).as_secs_f64() * 1e3);
        // A random gap, so triggers do not lock in step with the node's poll interval.
        let jitter = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::from(d.subsec_nanos()) / 1000 % 2500);
        tokio::time::sleep(Duration::from_millis(jitter)).await;
    }
    stats("state change, from trigger start", since_start);
    stats("state change, from trigger return", since_end);
    let secs = t_run.elapsed().as_secs_f64();
    println!(
        "stream: {} bytes in {:.1}s during {} changes ({:.0} B/change)",
        s.bytes - base_bytes,
        secs,
        a.cycles * 2,
        (s.bytes - base_bytes) as f64 / f64::from(a.cycles * 2)
    );

    let mut rtts = Vec::new();
    for id in 1..=u64::from(a.previews) {
        let req = UiRequest {
            body: Some(ui_request_body::Body::Command(Request {
                request_id: id,
                command: Some(Command {
                    kind: Some(ck::Kind::GetPreview(ck::GetPreview {
                        pane_ref: Some(PaneRefMsg::from(&pane)),
                        lines: 200,
                    })),
                }),
            })),
        };
        let t = Instant::now();
        s.client.send(req).map_err(|e| e.to_string())?;
        loop {
            let event = s.client.next_event().await.map_err(|e| e.to_string())?;
            let Some(event) = event else {
                return Err("stream closed during preview".into());
            };
            s.bytes += event.encoded_len() as u64;
            if let Some(ui_event_body::Body::Response(r)) = &event.body {
                if r.request_id == id {
                    break;
                }
            } else {
                let _: Step = s.image.apply(&event);
            }
        }
        rtts.push(t.elapsed().as_secs_f64() * 1e3);
    }
    stats("preview round trip", rtts);
    let _ = UiEvent::default();
    Ok(())
}

/// `flight-load watch --ui-dir DIR [--secs N]`: print every change to node liveness, pane
/// counts and the link, with milliseconds since start, reconnecting like a dashboard does.
async fn watch() {
    let mut ui_dir = PathBuf::new();
    let mut secs = 3600u64;
    let mut it = std::env::args().skip(2);
    while let Some(f) = it.next() {
        match (f.as_str(), it.next()) {
            ("--ui-dir", Some(v)) => ui_dir = v.into(),
            ("--secs", Some(v)) => secs = v.parse().unwrap_or(secs),
            _ => {
                eprintln!("usage: flight-load watch --ui-dir DIR [--secs N]");
                std::process::exit(2);
            }
        }
    }
    let config = match ClientConfig::load(&ui_dir.join("ui")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let start = Instant::now();
    let stamp = || format!("+{:>8.3}s", start.elapsed().as_secs_f64());
    let mut image = FleetImage::new();
    let mut last = String::new();
    let end = start + Duration::from_secs(secs);
    while Instant::now() < end {
        let mut client = match UiClient::connect(
            &config.address,
            &config.identity,
            &config.orchestrator,
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                let line = format!("link down: {e}");
                if line != last {
                    println!("{} {line}", stamp());
                    last = line;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
        };
        println!("{} link up", stamp());
        let _ = client.send(UiRequest {
            body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
        });
        loop {
            let ev = tokio::time::timeout(end - Instant::now().min(end), client.next_event()).await;
            match ev {
                Ok(Ok(Some(ev))) => {
                    if image.apply(&ev) == Step::Resync {
                        println!("{} resync", stamp());
                        break;
                    }
                    let line = image
                        .nodes()
                        .values()
                        .map(|n| {
                            let states: String = n
                                .panes
                                .values()
                                .map(|p| match StateCode::try_from(p.state) {
                                    Ok(StateCode::Permit) => 'P',
                                    Ok(StateCode::Busy) => 'B',
                                    Ok(StateCode::Done) => 'D',
                                    Ok(StateCode::Idle) => 'i',
                                    _ => '?',
                                })
                                .collect();
                            format!("{}={:?}[{}]", n.display_name, n.status_code(), states)
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    if line != last {
                        println!("{} {line}", stamp());
                        last = line;
                    }
                }
                Ok(Ok(None)) | Ok(Err(_)) => {
                    println!("{} link lost", stamp());
                    image.disconnected();
                    last.clear();
                    break;
                }
                Err(_) => return,
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
