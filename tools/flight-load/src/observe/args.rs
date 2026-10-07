// SPDX-License-Identifier: MIT

use std::time::Duration;

/// How captures are carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// One `tmux capture-pane` subprocess at a time (what the node does today).
    Sequential,
    /// Up to `n` subprocesses at once.
    Concurrent(usize),
    /// Every command over one persistent `tmux -C` connection.
    Control,
}

/// Which panes are captured each round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Every pane, every round.
    All,
    /// Only panes whose `window_activity` is not older than the second of their last capture.
    SkipUnchanged,
    /// Only panes that produced `%output` since their last capture (control mode only).
    Events,
}

#[derive(Debug)]
pub struct Args {
    pub socket: String,
    pub capture: Capture,
    pub policy: Policy,
    pub interval: Duration,
    pub secs: u64,
    pub settle_rounds: u32,
}

impl Args {
    pub fn parse(mut it: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut a = Args {
            socket: String::new(),
            capture: Capture::Sequential,
            policy: Policy::All,
            interval: Duration::from_millis(1000),
            secs: 30,
            settle_rounds: 3,
        };
        while let Some(flag) = it.next() {
            let v = it.next().ok_or(format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--socket" => a.socket = v,
                "--capture" => a.capture = parse_capture(&v)?,
                "--policy" => a.policy = parse_policy(&v)?,
                "--interval-ms" => a.interval = Duration::from_millis(num(&flag, &v)?),
                "--secs" => a.secs = num(&flag, &v)?,
                "--settle" => a.settle_rounds = u32::try_from(num(&flag, &v)?).unwrap_or(3),
                other => return Err(format!("unknown flag {other}")),
            }
        }
        if a.socket.is_empty() {
            return Err("usage: flight-load observe --socket NAME [--capture seq|conc:K|ctl] [--policy all|skip|events] [--interval-ms MS] [--secs S] [--settle ROUNDS]".into());
        }
        if a.policy == Policy::Events && a.capture != Capture::Control {
            return Err("--policy events needs --capture ctl".into());
        }
        Ok(a)
    }
}

fn num(flag: &str, v: &str) -> Result<u64, String> {
    v.parse().map_err(|_| format!("bad {flag} {v:?}"))
}

fn parse_capture(v: &str) -> Result<Capture, String> {
    match v {
        "seq" => Ok(Capture::Sequential),
        "ctl" => Ok(Capture::Control),
        other => match other.strip_prefix("conc:").map(str::parse::<usize>) {
            Some(Ok(n)) if (1..=64).contains(&n) => Ok(Capture::Concurrent(n)),
            _ => Err(format!("bad --capture {v:?}: seq, conc:1..64 or ctl")),
        },
    }
}

fn parse_policy(v: &str) -> Result<Policy, String> {
    match v {
        "all" => Ok(Policy::All),
        "skip" => Ok(Policy::SkipUnchanged),
        "events" => Ok(Policy::Events),
        _ => Err(format!("bad --policy {v:?}: all, skip or events")),
    }
}
