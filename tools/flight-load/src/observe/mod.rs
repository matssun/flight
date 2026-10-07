// SPDX-License-Identifier: MIT

//! `flight-load observe`: how fast, and at what cost, can a node observe N tmux panes?
//!
//! A strategy is a capture transport (sequential subprocesses, bounded-concurrent
//! subprocesses, one control-mode connection) plus a policy (capture everything, skip panes
//! whose `window_activity` has not moved). Event-driven capture (`%output`) was tried and
//! rejected: a control client only receives it for panes of its own session.
//! Each round prints one JSON line: when it ended, how long it took, how many captures it
//! made, and the `rev N` marker each pane showed, so a driver can compare detection latency
//! and correctness against what it wrote.

mod args;
mod control;
#[cfg(test)]
mod live_tests;
mod observer;
mod protocol;
mod subprocess;
mod transport;

use args::{Args, Capture};
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
        Capture::Control => drive(&args, Control::start(&args.socket)?),
    }
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
        let last = step(&mut observer, started);
        if let Err(e) = &last {
            // The observer already dropped what it believed; rebuild the transport and start
            // from a full refresh next round.
            eprintln!("observe: round failed: {e}");
            if let Err(e) = observer.recover() {
                eprintln!("observe: recover failed: {e}");
            }
        }
        if Instant::now() >= end {
            if settle == 0 {
                return last;
            }
            settle -= 1;
        }
        std::thread::sleep(args.interval.saturating_sub(started.elapsed()));
    }
}

fn step<T: Transport>(observer: &mut Observer<T>, started: Instant) -> Result<(), String> {
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
    Ok(())
}
