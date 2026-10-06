// SPDX-License-Identifier: MIT

use super::*;
use crate::TmuxOutput;
use std::cell::RefCell;

struct Fake {
    calls: RefCell<Vec<Vec<String>>>,
    reply: Result<String, i32>,
}

impl Fake {
    fn ok(s: &str) -> Self {
        Self {
            calls: RefCell::default(),
            reply: Ok(s.into()),
        }
    }
    fn failing() -> Self {
        Self {
            calls: RefCell::default(),
            reply: Err(1),
        }
    }
}

impl TmuxRunner for &Fake {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        match &self.reply {
            Ok(s) => Ok(TmuxOutput { stdout: s.clone() }),
            Err(c) => Err(TmuxError::Failed {
                code: Some(*c),
                stderr: "no server".into(),
            }),
        }
    }
}

#[test]
fn list_panes_passes_format_and_parses() {
    let f = Fake::ok("%1\ts\tw\t@1\t0\t/\t7\t1\t1\t1\tt\n");
    let panes = Tmux::with_runner(&f).list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(f.calls.borrow()[0][..3], ["list-panes", "-a", "-F"]);
}

#[test]
fn list_panes_surfaces_failure() {
    assert!(Tmux::with_runner(&Fake::failing()).list_panes().is_err());
}

#[test]
fn capture_pane_builds_flags() {
    let f = Fake::ok("hi");
    let out = Tmux::with_runner(&f)
        .capture_pane("%2", true, Some(50))
        .unwrap();
    assert_eq!(out, "hi");
    assert_eq!(
        f.calls.borrow()[0],
        ["capture-pane", "-p", "-t", "%2", "-e", "-S", "-50"]
    );
}

#[test]
fn session_targets_use_exact_match() {
    let f = Fake::ok("");
    let t = Tmux::with_runner(&f);
    assert!(t.has_session("api"));
    t.kill_session("api").unwrap();
    assert_eq!(f.calls.borrow()[0], ["has-session", "-t", "=api"]);
    assert_eq!(f.calls.borrow()[1], ["kill-session", "-t", "=api"]);
}

#[test]
fn has_session_false_on_failure() {
    assert!(!Tmux::with_runner(&Fake::failing()).has_session("x"));
}
