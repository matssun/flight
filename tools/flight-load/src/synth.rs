// SPDX-License-Identifier: MIT

//! `flight-load synth`: a real node (its own enrolled identity, the real `NodeLink` and
//! `NodeCore`, including classification) whose "tmux" is N scripted panes, plus an in-process
//! UI subscriber that times flip -> visible-in-the-UI on a single clock.
//!
//! This measures the control plane (observe -> classify -> delta -> orchestrator -> UI), not
//! tmux: the node-side polling cost is measured with real panes instead.

use flight_classify::AgentKind;
use flight_client::ClientConfig;
use flight_node::{
    fresh_incarnation, Control, ControlError, NodeCore, NodeSession, PaneObservation, Round,
    ServerOutcome,
};
use flight_proto::{ui_request_body, FleetImage, StateCode, Step, Subscribe, UiRequest};
use flight_state::{PaneId, ServerId};
use flight_transport::{config_path, identity_dir, NodeLink, NodeLinkConfig, UiClient};
use flight_trust::{ConnectionConfig, Identity};
use prost::Message;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PERMIT: &str = include_str!("../../../flight-classify/tests/fixtures/claude-permit.txt");
const IDLE: &str = "Done!\n\n❯\n";
const NAME: &str = "synth";

pub struct Args {
    pub node_dir: PathBuf,
    pub ui_dir: PathBuf,
    pub panes: usize,
    pub interval: Duration,
    pub flips: usize,
    pub secs: u64,
    /// The orchestrator's config dir: when given, the synthetic node is forgotten when the
    /// run ends, so test nodes never linger on a dashboard.
    pub orch_dir: Option<PathBuf>,
}

impl Args {
    pub fn parse(mut it: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut a = Args {
            node_dir: PathBuf::new(),
            ui_dir: PathBuf::new(),
            panes: 10,
            interval: Duration::from_millis(500),
            flips: 1,
            secs: 30,
            orch_dir: None,
        };
        while let Some(flag) = it.next() {
            let v = it.next().ok_or(format!("{flag} needs a value"))?;
            let num = |s: &str| s.parse::<u64>().map_err(|_| format!("bad {flag}"));
            match flag.as_str() {
                "--node-dir" => a.node_dir = v.into(),
                "--ui-dir" => a.ui_dir = v.into(),
                "--panes" => a.panes = num(&v)? as usize,
                "--interval-ms" => a.interval = Duration::from_millis(num(&v)?),
                "--flips" => a.flips = num(&v)? as usize,
                "--secs" => a.secs = num(&v)?,
                "--orch-dir" => a.orch_dir = Some(v.into()),
                other => return Err(format!("unknown flag {other}")),
            }
        }
        if a.node_dir.as_os_str().is_empty() || a.ui_dir.as_os_str().is_empty() {
            return Err("usage: flight-load synth --node-dir DIR --ui-dir DIR [--panes N] [--interval-ms MS] [--flips K] [--secs S] [--orch-dir DIR]".into());
        }
        Ok(a)
    }
}

struct NoControl;

impl Control for NoControl {
    fn capture(&self, _: &ServerId, pane: &PaneId, _: u32) -> Result<String, ControlError> {
        Ok(format!("synthetic {pane}\n"))
    }
    fn kill_pane(&self, _: &ServerId, _: &PaneId, _: u32) -> Result<(), ControlError> {
        Ok(())
    }
}

/// A tiny xorshift: enough to pick panes without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[derive(Default)]
struct Pending {
    /// session -> (wants Permit, when it was flipped)
    waiting: HashMap<String, (bool, Instant)>,
    latencies_ms: Vec<f64>,
    bytes: u64,
    events: u64,
    resyncs: u64,
    /// Panes of the synthetic node the UI image currently shows (0 while the link is down).
    visible: usize,
}

pub async fn run(a: Args) -> Result<(), String> {
    let node_role = a.node_dir.join("node");
    let cfg = ConnectionConfig::load(&config_path(&node_role)).map_err(|e| e.to_string())?;
    let identity = Arc::new(Identity::load(&identity_dir(&node_role)).map_err(|e| e.to_string())?);
    let node_id = identity.fingerprint().as_str().to_owned();
    let session = NodeSession::new(
        NodeCore::new(
            identity.fingerprint().host_id(),
            fresh_incarnation().map_err(|e| e.to_string())?,
        ),
        NAME.to_owned(),
    );
    let link = Arc::new(NodeLink::new(
        NodeLinkConfig {
            address: cfg.address.clone(),
            orchestrator: cfg.orchestrator().map_err(|e| e.to_string())?,
            identity,
            servers: vec!["flight".to_owned()],
            heartbeat_interval: Duration::from_secs(5),
            reconnect_min: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(10),
        },
        session,
        Arc::new(NoControl),
    ));
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let conn = tokio::spawn({
        let link = link.clone();
        async move { link.run(stop_rx).await }
    });

    let shared = Arc::new(Mutex::new(Pending::default()));
    let ui = ClientConfig::load(&a.ui_dir.join("ui")).map_err(|e| e.to_string())?;
    let ui_task = tokio::spawn(ui_side(ui, shared.clone()));
    // Print every change in how many of the node's panes the UI can see, with the wall clock,
    // so a restart's convergence time can be read off the log.
    let watcher = tokio::spawn({
        let shared = shared.clone();
        async move {
            let mut last = usize::MAX;
            loop {
                let seen = lock(&shared).visible;
                if seen != last {
                    let secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0.0, |d| d.as_secs_f64());
                    println!("visible {secs:.3} {seen}");
                    last = seen;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    });

    let mut permit = vec![false; a.panes];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ a.panes as u64);
    let end = Instant::now() + Duration::from_secs(a.secs);
    let mut tick = 0u64;
    let mut flips_total = 0u64;
    while Instant::now() < end {
        // After the first tick, flip K distinct panes, noting when.
        if tick > 0 {
            let mut chosen: Vec<usize> = Vec::new();
            if a.flips >= a.panes {
                chosen.extend(0..a.panes);
            }
            while chosen.len() < a.flips.min(a.panes) {
                let i = (rng.next() % a.panes as u64) as usize;
                if !chosen.contains(&i) {
                    chosen.push(i);
                }
            }
            let mut p = lock(&shared);
            for i in chosen {
                permit[i] = !permit[i];
                // A flip of a pane still pending replaces its timer: measure the latest.
                p.waiting
                    .insert(format!("s{i}"), (permit[i], Instant::now()));
                flips_total += 1;
            }
        }
        let panes = (0..a.panes)
            .map(|i| PaneObservation {
                pane: PaneId::new(format!("%{i}")),
                pid: 1000 + i as u32,
                agent: AgentKind::Claude,
                session: format!("s{i}"),
                window: "w".to_owned(),
                path: "/tmp".to_owned(),
                command: "claude".to_owned(),
                title: String::new(),
                focused: false,
                screen_lines: if permit[i] { PERMIT } else { IDLE }
                    .lines()
                    .map(str::to_owned)
                    .collect(),
                placement: Default::default(),
            })
            .collect();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let t = Instant::now();
        link.observe(vec![Round {
            server: ServerId::new("flight"),
            now,
            outcome: ServerOutcome::Observed(panes),
        }]);
        if tick == 0 || tick.is_multiple_of(20) {
            eprintln!(
                "tick {tick}: observe of {} panes took {:.2} ms",
                a.panes,
                t.elapsed().as_secs_f64() * 1e3
            );
        }
        tick += 1;
        tokio::time::sleep(a.interval).await;
    }
    // Give the last flips time to land.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (mut lat, bytes, events, resyncs, unresolved) = {
        let p = lock(&shared);
        (
            p.latencies_ms.clone(),
            p.bytes,
            p.events,
            p.resyncs,
            p.waiting.len(),
        )
    };
    let backlog_note = "see `flight orchestrator trust status` for backlog";
    println!(
        "panes={} interval={}ms flips/tick={} duration={}s",
        a.panes,
        a.interval.as_millis(),
        a.flips,
        a.secs
    );
    println!(
        "flips={flips_total} seen={} unresolved={unresolved} ui_events={events} ui_bytes={bytes} resyncs={resyncs}",
        lat.len()
    );
    lat.sort_by(|x, y| x.total_cmp(y));
    if !lat.is_empty() {
        let q = |f: f64| lat[((lat.len() as f64 - 1.0) * f).round() as usize];
        println!(
            "flip -> UI: p50={:.1} p90={:.1} p99={:.1} max={:.1} (ms)",
            q(0.5),
            q(0.9),
            q(0.99),
            lat[lat.len() - 1]
        );
    }
    println!("{backlog_note}");
    let _ = stop.send(true);
    let _ = conn.await;
    ui_task.abort();
    watcher.abort();
    if let Some(dir) = &a.orch_dir {
        forget_self(dir, &node_id).await;
    }
    Ok(())
}

fn lock(m: &Mutex<Pending>) -> std::sync::MutexGuard<'_, Pending> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The UI side: subscribe, keep an image, and resolve pending flips as they become visible.
async fn ui_side(ui: ClientConfig, shared: Arc<Mutex<Pending>>) {
    loop {
        let Ok(mut client) = UiClient::connect(&ui.address, &ui.identity, &ui.orchestrator).await
        else {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };
        let _ = client.send(UiRequest {
            body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
        });
        let mut image = FleetImage::new();
        loop {
            let Ok(Some(event)) = client.next_event().await else {
                lock(&shared).visible = 0;
                break;
            };
            let step = image.apply(&event);
            let mut p = lock(&shared);
            p.visible = image
                .nodes()
                .values()
                .find(|n| n.display_name == NAME)
                .map_or(0, |n| n.panes.len());
            p.bytes += event.encoded_len() as u64;
            p.events += 1;
            if step == Step::Resync {
                p.resyncs += 1;
                break;
            }
            if p.waiting.is_empty() {
                continue;
            }
            let Some(node) = image.nodes().values().find(|n| n.display_name == NAME) else {
                continue;
            };
            let states: HashMap<&str, bool> = node
                .panes
                .values()
                .map(|x| (x.session.as_str(), x.state == StateCode::Permit as i32))
                .collect();
            let now = Instant::now();
            let done: Vec<String> = p
                .waiting
                .iter()
                .filter(|(s, (want, _))| states.get(s.as_str()) == Some(want))
                .map(|(s, _)| s.clone())
                .collect();
            for s in done {
                if let Some((_, t)) = p.waiting.remove(&s) {
                    p.latencies_ms.push((now - t).as_secs_f64() * 1e3);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Ask the orchestrator to forget this (now disconnected) synthetic node. It may take a
/// moment for the orchestrator to notice the disconnect, so retry briefly.
async fn forget_self(orch_dir: &std::path::Path, node_id: &str) {
    let socket = orch_dir.join("orchestrator").join("admin.sock");
    for _ in 0..20 {
        match flight_transport::admin_request(&socket, &format!("forget {node_id}")).await {
            Ok(reply) => {
                println!("{reply}");
                return;
            }
            Err(e) if e.to_string().contains("still connected") => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(e) => {
                eprintln!("could not forget the synthetic node: {e}");
                return;
            }
        }
    }
    eprintln!("the synthetic node was still connected; it was not forgotten");
}
