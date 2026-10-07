// SPDX-License-Identifier: MIT

//! `flight-load observe`: how fast, and at what cost, can a node observe N tmux panes?
//!
//! A strategy is a capture transport (sequential subprocesses, bounded-concurrent
//! subprocesses, one control-mode connection) plus a policy (capture everything, skip panes
//! whose `window_activity` has not moved, or capture only panes that produced `%output`).
//! Each round prints one JSON line: when it ended, how long it took, how many captures it
//! made, and the `rev N` marker each pane showed, so a driver can compare detection latency
//! and correctness against what it wrote.

mod args;
mod control;
mod observer;
mod subprocess;
mod transport;

use args::{Args, Capture, Policy};
use control::Control;
use observer::Observer;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subprocess::Subprocess;
use transport::Transport;

pub fn main(args: impl Iterator<Item = String>) -> Result<(), String> {
    let args = Args::parse(args)?;
    match args.capture {
        Capture::Sequential => drive(&args, Subprocess::new(&args.socket, 1)),
        Capture::Concurrent(n) => drive(&args, Subprocess::new(&args.socket, n)),
        Capture::Control => {
            let session = first_session(&args.socket)?;
            let control = Control::start(&args.socket, &session, args.policy == Policy::Events)?;
            drive(&args, control)
        }
    }
}

fn first_session(socket: &str) -> Result<String, String> {
    let out = std::process::Command::new("tmux")
        .args(["-u", "-L", socket, "list-sessions", "-F", "#{session_name}"])
        .output()
        .map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "no tmux session to attach to".to_owned())
}

/// The newest `rev N` marker in a screen (the workload writes one), or -1.
fn rev_of(screen: &str) -> i64 {
    // The last marker: the capture includes scrollback, which holds older ones.
    screen
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("rev "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(-1)
}

fn drive<T: Transport>(args: &Args, transport: T) -> Result<(), String> {
    let mut observer = Observer::new(transport, args.policy);
    let end = Instant::now() + Duration::from_secs(args.secs);
    let mut settle = args.settle_rounds;
    loop {
        let started = Instant::now();
        let round = observer.round()?;
        let took = started.elapsed();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let revs: Vec<String> = round
            .screens
            .iter()
            .map(|(id, s)| format!("\"{id}\":{}", rev_of(s)))
            .collect();
        println!(
            "{{\"t\":{now_ms},\"ms\":{:.2},\"captured\":{},\"panes\":{},\"revs\":{{{}}}}}",
            took.as_secs_f64() * 1e3,
            round.captured,
            round.screens.len(),
            revs.join(",")
        );
        if Instant::now() >= end {
            if settle == 0 {
                return Ok(());
            }
            settle -= 1;
        }
        std::thread::sleep(args.interval.saturating_sub(started.elapsed()));
    }
}
