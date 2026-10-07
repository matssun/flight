// SPDX-License-Identifier: MIT

//! A soak: a real tmux server under churn and abuse, the control observer compared with the
//! reference after every step. Ignored by default; run it as long as you like:
//!
//! `FLIGHT_SOAK_SECS=1800 FLIGHT_SOAK_PANES=60 cargo test -p flight-node --test control_skip_soak -- --ignored --nocapture`
//!
//! Faults injected at random: typing into panes, creating and killing panes, killing the
//! control client with SIGKILL, and killing the whole tmux server. After each step the two
//! observers must agree (focus excluded: only our own client is attached) within a short grace
//! period; any step where they do not is a failure.

use flight_node::{
    ControlLink, ControlSkipObserver, PaneObserver, SequentialObserver, ServerOutcome, TmuxServers,
};
use flight_state::ServerId;
use flight_tmux::{ControlConnection, SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

const GRACE: Duration = Duration::from_secs(10);

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Everything but `focused`, in a comparable form.
fn view(rounds: &[flight_node::Round]) -> String {
    match &rounds[0].outcome {
        ServerOutcome::Observed(p) => {
            let mut p = p.clone();
            p.iter_mut().for_each(|x| x.focused = false);
            format!("{p:?}")
        }
        other => format!("{other:?}"),
    }
}

#[test]
#[ignore = "soak: set FLIGHT_SOAK_SECS; runs for that long"]
fn the_control_observer_survives_churn_and_faults() {
    let Ok(cat) = Command::new("which").arg("cat").output() else {
        return;
    };
    if Command::new("tmux").arg("-V").output().is_err() {
        return;
    }
    let secs = env("FLIGHT_SOAK_SECS", 30);
    let target_panes = env("FLIGHT_SOAK_PANES", 40) as usize;
    let dir = std::env::temp_dir().join(format!("flight-test-{}-soak", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let claude = dir.join("claude");
    std::fs::copy(String::from_utf8(cat.stdout).unwrap().trim(), &claude).unwrap();
    if cfg!(target_os = "macos") {
        Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(&claude)
            .output()
            .unwrap();
    }
    let endpoint =
        TmuxEndpoint::named(&format!("flight-test-{}-soak", std::process::id())).unwrap();
    let tmux = Tmux::new(endpoint.clone());
    let command = claude.to_string_lossy().into_owned();

    let mut servers = TmuxServers::new();
    servers.add(
        ServerId::new("soak"),
        Box::new(SystemRunner::new(endpoint.clone())),
    );
    let servers = Arc::new(servers);
    let mut reference = SequentialObserver::new(servers.clone());
    let mut control = ControlSkipObserver::new(servers);
    let watched = endpoint.clone();
    control.watch(ServerId::new("soak"), move || {
        Ok(Box::new(ControlConnection::open(&watched)?) as Box<dyn ControlLink>)
    });

    let mut rng = Rng(0x2545_F491_4F6C_DD1D ^ std::process::id() as u64);
    let (mut next_name, mut steps, mut grace_waits, mut failures) = (0u64, 0u64, 0u64, Vec::new());
    let mut faults = [0u64; 4];
    let mut notes = Vec::new();
    let end = Instant::now() + Duration::from_secs(secs);
    let mut live: Vec<String> = Vec::new();

    while Instant::now() < end {
        steps += 1;
        match rng.next(100) {
            0..=44 if !live.is_empty() => {
                let name = &live[rng.next(live.len() as u64) as usize];
                let _ = tmux.runner().run(&[
                    "send-keys",
                    "-t",
                    &format!("={name}:"),
                    &format!("line {steps}"),
                    "Enter",
                ]);
            }
            45..=59 if live.len() < target_panes => {
                next_name += 1;
                let name = format!("p{next_name}");
                if tmux.new_session_running(&name, "/tmp", &command).is_ok() {
                    live.push(name);
                }
            }
            60..=69 if live.len() > 1 => {
                let name = live.swap_remove(rng.next(live.len() as u64) as usize);
                let _ = tmux.kill_session(&name);
            }
            70..=72 => {
                // SIGKILL the control client: the next round must notice and recover.
                if let Ok(out) = tmux.runner().run(&["list-clients", "-F", "#{client_pid}"]) {
                    for pid in out.stdout.lines() {
                        let _ = Command::new("kill").args(["-9", pid]).status();
                        faults[0] += 1;
                    }
                }
            }
            73 if rng.next(8) == 0 => {
                // The whole server goes away and comes back with different processes.
                let _ = tmux.kill_server();
                live.clear();
                faults[1] += 1;
            }
            _ => {}
        }
        if live.is_empty() {
            next_name += 1;
            let name = format!("p{next_name}");
            if tmux.new_session_running(&name, "/tmp", &command).is_ok() {
                live.push(name);
            }
        }
        std::thread::sleep(Duration::from_millis(20 + rng.next(120)));

        // The two observers must agree, allowing the grace period for the pane's program to
        // have produced its output and for a recovering connection to be back.
        let started = Instant::now();
        let mut agreed = false;
        while started.elapsed() < GRACE {
            let (want, got) = (reference.observe(now()), control.observe(now()));
            if view(&want) == view(&got) {
                agreed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if started.elapsed() > Duration::from_millis(400) {
            grace_waits += 1;
        }
        if !agreed {
            failures.push(format!("step {steps}: still different after {GRACE:?}"));
            if failures.len() >= 5 {
                break;
            }
        }
        notes.extend(control.take_notes());
    }

    let _ = tmux.kill_server();
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "soak: {steps} steps in {secs}s, {} panes alive at the end, {} control-client kills, {} server kills, {grace_waits} steps needed >400 ms to agree, {} operator notes",
        live.len(),
        faults[0],
        faults[1],
        notes.len()
    );
    for n in notes.iter().take(6) {
        println!("  note: {n}");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
