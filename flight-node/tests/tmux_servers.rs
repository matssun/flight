// SPDX-License-Identifier: MIT

//! The tmux adapter over scripted runners: agent detection, capture failures, per-server
//! failure isolation, and the full path into NodeCore.

mod support;

use flight_node::{Control, ServerOutcome, TmuxServers, Unavailable};
use flight_proto::{delta_change::Change, ErrorKindCode};
use flight_state::{AgentState, ServerId};
use flight_tmux::{TmuxError, TmuxOutput, TmuxRunner};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use support::*;

struct Fake {
    panes: String,
    screens: Arc<Mutex<HashMap<String, String>>>,
    list_error: Option<TmuxError>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl TmuxRunner for Fake {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        self.calls.lock().unwrap().push(args.join(" "));
        match args.first().copied() {
            Some("list-panes") => match &self.list_error {
                Some(TmuxError::Failed { code, stderr }) => Err(TmuxError::Failed {
                    code: *code,
                    stderr: stderr.clone(),
                }),
                Some(TmuxError::Spawn(e)) => {
                    Err(TmuxError::Spawn(std::io::Error::new(e.kind(), "x")))
                }
                Some(_) => Err(TmuxError::InvalidEndpoint("x".into())),
                None => Ok(TmuxOutput {
                    stdout: self.panes.clone(),
                }),
            },
            Some("capture-pane") => {
                let target = args
                    .iter()
                    .position(|a| *a == "-t")
                    .and_then(|i| args.get(i + 1))
                    .copied()
                    .unwrap_or("");
                match self.screens.lock().unwrap().get(target) {
                    Some(s) => Ok(TmuxOutput { stdout: s.clone() }),
                    None => Err(TmuxError::Failed {
                        code: Some(1),
                        stderr: "no pane".into(),
                    }),
                }
            }
            _ => Ok(TmuxOutput {
                stdout: String::new(),
            }),
        }
    }
}

fn row(id: &str, command: &str, pid: u32) -> String {
    format!("{id}\twork\tw\t@1\t0\t/tmp\t{pid}\t0\t0\t0\t{command}\t1700\t0\ttitle\n")
}

fn fake(panes: String, screens: &[(&str, &str)], list_error: Option<TmuxError>) -> Box<Fake> {
    Box::new(Fake {
        panes,
        screens: Arc::new(Mutex::new(
            screens
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )),
        list_error,
        calls: Arc::default(),
    })
}

fn observed(outcome: &ServerOutcome) -> &[flight_node::PaneObservation] {
    match outcome {
        ServerOutcome::Observed(p) => p,
        other => panic!("not observed: {other:?}"),
    }
}

#[test]
fn agents_are_observed_and_shells_are_not() {
    let mut t = TmuxServers::new();
    let panes = [
        row("%1", "claude", 11),
        row("%2", "zsh", 12),
        row("%3", "2.1.138", 13),
    ]
    .concat();
    t.add(
        server(),
        fake(panes, &[("%1", PERMIT_SCREEN), ("%3", IDLE_SCREEN)], None),
    );
    let rounds = t.observe(100);
    let seen = observed(&rounds[0].outcome);
    let ids: Vec<&str> = seen.iter().map(|p| p.pane.as_str()).collect();
    assert_eq!(ids, vec!["%1", "%3"]);
    assert_eq!(seen[0].pid, 11);
    assert!(seen[0]
        .screen_lines
        .iter()
        .any(|l| l.contains("Do you want")));
}

#[test]
fn a_failed_capture_keeps_the_pane_with_no_screen_evidence() {
    let mut t = TmuxServers::new();
    t.add(server(), fake(row("%1", "claude", 11), &[], None));
    let rounds = t.observe(100);
    let seen = observed(&rounds[0].outcome);
    assert_eq!(seen.len(), 1);
    assert!(seen[0].screen_lines.is_empty());
}

#[test]
fn failures_are_typed_and_isolated_per_server() {
    let mut t = TmuxServers::new();
    t.add(
        ServerId::new("a-ok"),
        fake(row("%1", "claude", 1), &[("%1", PERMIT_SCREEN)], None),
    );
    t.add(
        ServerId::new("b-empty"),
        fake(
            String::new(),
            &[],
            Some(TmuxError::Failed {
                code: Some(1),
                stderr: "no server running on /tmp/x".into(),
            }),
        ),
    );
    t.add(
        ServerId::new("c-missing"),
        fake(
            String::new(),
            &[],
            Some(TmuxError::Spawn(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            ))),
        ),
    );
    t.add(
        ServerId::new("d-odd"),
        fake(
            String::new(),
            &[],
            Some(TmuxError::Failed {
                code: Some(1),
                stderr: "boom".into(),
            }),
        ),
    );
    let rounds = t.observe(100);
    assert!(matches!(rounds[0].outcome, ServerOutcome::Observed(_)));
    assert_eq!(
        rounds[1].outcome,
        ServerOutcome::Unavailable(Unavailable::NoServer)
    );
    assert_eq!(
        rounds[2].outcome,
        ServerOutcome::Unavailable(Unavailable::TmuxMissing)
    );
    assert!(
        matches!(&rounds[3].outcome, ServerOutcome::Unavailable(Unavailable::Failed(m)) if m.contains("boom"))
    );
}

#[test]
fn adapter_into_core_publishes_state_for_the_observed_panes() {
    let mut t = TmuxServers::new();
    t.add(
        server(),
        fake(row("%1", "claude", 11), &[("%1", PERMIT_SCREEN)], None),
    );
    let mut c = core();
    let deltas: Vec<_> = t
        .observe(100)
        .into_iter()
        .flat_map(|r| c.apply(r))
        .collect();
    let state = deltas
        .iter()
        .find_map(|d| match d.change.as_ref() {
            Some(Change::PaneUpsert(p)) => Some(p),
            _ => None,
        })
        .expect("a pane upsert");
    assert_eq!(state.agent_state(), Ok(AgentState::Permit));
}

#[test]
fn kill_targets_the_published_process_not_just_the_pane_id() {
    let mut t = TmuxServers::new();
    // The pane table says %1 is now pid 99; the request was issued against pid 11.
    let fake = fake(row("%1", "claude", 99), &[], None);
    let calls = fake.calls.clone();
    t.add(server(), fake);
    let stale = t
        .kill_pane(&server(), &flight_state::PaneId::new("%1"), 11)
        .unwrap_err();
    assert_eq!(stale.kind, ErrorKindCode::UnknownPane);
    assert!(
        !calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("kill-pane")),
        "a stale request must not reach kill-pane"
    );
    t.kill_pane(&server(), &flight_state::PaneId::new("%1"), 99)
        .expect("same process");
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("kill-pane")));
}

#[test]
fn control_errors_are_typed() {
    let mut t = TmuxServers::new();
    t.add(server(), fake(String::new(), &[], None));
    let err = t
        .capture(&ServerId::new("nope"), &flight_state::PaneId::new("%1"), 5)
        .unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::InvalidRequest);
    let err = t
        .capture(&server(), &flight_state::PaneId::new("%1"), 5)
        .unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::RemoteCommandFailed);
}
