// SPDX-License-Identifier: MIT

//! The collector over scripted runners: agents are detected, screens classified, state
//! resolved over time, and one down host never fails the refresh.

use flight_control::{BoxedRunner, HostRegistry, Transport};
use flight_state::AgentState::{Busy, Done, Idle, Permit};
use flight_state::{HostId, ServerId};
use flight_tmux::{TmuxEndpoint, TmuxError, TmuxOutput, TmuxRunner};
use flight_ui::{Collector, HostHealth};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const PERMIT_SCREEN: &str = include_str!("../../flight-classify/tests/fixtures/claude-permit.txt");
const IDLE_SCREEN: &str = "Done!\n\n❯\n";
const BUSY_SCREEN: &str = "✻ Trapping Gollum… (8s · ↑ 240 tokens)\n\n❯\n";

/// Answers list-panes with a fixed pane table and capture-pane from a mutable per-pane map.
struct FakeTmux {
    panes: Arc<Mutex<String>>,
    screens: Arc<Mutex<HashMap<String, String>>>,
    fail: Option<(i32, String)>,
}

impl TmuxRunner for FakeTmux {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        if let Some((code, stderr)) = &self.fail {
            return Err(TmuxError::Failed {
                code: Some(*code),
                stderr: stderr.clone(),
            });
        }
        match args.first().copied() {
            Some("list-panes") => Ok(TmuxOutput {
                stdout: self.panes.lock().unwrap().clone(),
            }),
            Some("capture-pane") => {
                let target = args
                    .iter()
                    .position(|a| *a == "-t")
                    .and_then(|i| args.get(i + 1))
                    .copied()
                    .unwrap_or("");
                let screens = self.screens.lock().unwrap();
                Ok(TmuxOutput {
                    stdout: screens.get(target).cloned().unwrap_or_default(),
                })
            }
            _ => Ok(TmuxOutput {
                stdout: String::new(),
            }),
        }
    }
}

fn pane_row(id: &str, session: &str, command: &str, focused: bool) -> String {
    let f = if focused { "1" } else { "0" };
    format!("{id}\t{session}\tw\t@1\t0\t/tmp\t7\t{f}\t{f}\t{f}\t{command}\t1700\t0\ttitle\n")
}

type Screens = Arc<Mutex<HashMap<String, String>>>;

fn registry(panes: String, screens: &Screens) -> HostRegistry {
    let mut r = HostRegistry::new();
    r.add_host(HostId::new("mini-1"), Transport::Local);
    let runner: BoxedRunner = Box::new(FakeTmux {
        panes: Arc::new(Mutex::new(panes)),
        screens: screens.clone(),
        fail: None,
    });
    r.add_server_with_runner(
        &HostId::new("mini-1"),
        ServerId::new("flight"),
        TmuxEndpoint::named("flight").unwrap(),
        runner,
    )
    .unwrap();
    r
}

fn screens(entries: &[(&str, &str)]) -> Screens {
    Arc::new(Mutex::new(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    ))
}

#[test]
fn agents_are_detected_classified_and_shells_are_hidden() {
    let panes = [
        pane_row("%1", "nga", "claude", false),
        pane_row("%2", "scratch", "zsh", false),
        pane_row("%3", "old", "2.1.138", false),
    ]
    .concat();
    let sc = screens(&[
        ("%1", PERMIT_SCREEN),
        ("%3", IDLE_SCREEN),
        ("%2", PERMIT_SCREEN),
    ]);
    let mut c = Collector::new(registry(panes, &sc));
    let snap = c.collect(1000);
    let host = &snap.hosts[0];
    assert_eq!(host.health, HostHealth::Online);
    let by: HashMap<_, _> = host
        .panes
        .iter()
        .map(|p| (p.pane_ref.pane.as_str().to_owned(), p))
        .collect();
    assert_eq!(by.len(), 2, "the zsh pane is not an agent");
    assert_eq!(by["%1"].state, Permit);
    assert_eq!(by["%1"].why, "permit.do-you-want");
    assert_eq!(by["%3"].state, Idle, "a version-named binary is a Claude");
}

#[test]
fn finished_while_away_becomes_done_over_two_refreshes_and_clears_when_viewed() {
    let sc = screens(&[("%1", BUSY_SCREEN)]);
    let mut c = Collector::new(registry(pane_row("%1", "nga", "claude", false), &sc));
    assert_eq!(c.collect(1000).hosts[0].panes[0].state, Busy);
    sc.lock().unwrap().insert("%1".into(), IDLE_SCREEN.into());
    let done = c.collect(1010);
    assert_eq!(done.hosts[0].panes[0].state, Done);
    assert_eq!(done.hosts[0].panes[0].why, "finished while away");
    assert_eq!(c.collect(1020).hosts[0].panes[0].state, Done, "persists");
}

#[test]
fn finishing_while_focused_is_idle_not_done() {
    let sc = screens(&[("%1", BUSY_SCREEN)]);
    let mut c = Collector::new(registry(pane_row("%1", "nga", "claude", true), &sc));
    c.collect(1000);
    sc.lock().unwrap().insert("%1".into(), IDLE_SCREEN.into());
    assert_eq!(c.collect(1010).hosts[0].panes[0].state, Idle);
}

#[test]
fn one_unreachable_host_does_not_fail_the_refresh() {
    let sc = screens(&[("%1", PERMIT_SCREEN)]);
    let mut r = registry(pane_row("%1", "nga", "claude", false), &sc);
    r.add_host(
        HostId::new("mini-2"),
        Transport::Ssh {
            alias: "mini-2".into(),
        },
    );
    let down: BoxedRunner = Box::new(FakeTmux {
        panes: Arc::default(),
        screens: sc.clone(),
        fail: Some((
            255,
            "ssh: connect to host mini-2 port 22: Connection refused".into(),
        )),
    });
    r.add_server_with_runner(
        &HostId::new("mini-2"),
        ServerId::new("flight"),
        TmuxEndpoint::named("flight").unwrap(),
        down,
    )
    .unwrap();
    let snap = Collector::new(r).collect(1000);
    assert_eq!(snap.hosts.len(), 2);
    let by = |h: &str| snap.hosts.iter().find(|x| x.host.as_str() == h).unwrap();
    assert_eq!(by("mini-1").panes.len(), 1);
    assert!(matches!(by("mini-2").health, HostHealth::Unreachable(_)));
}

#[test]
fn a_pending_done_survives_a_transient_host_outage() {
    let sc = screens(&[("%1", BUSY_SCREEN)]);
    let panes = pane_row("%1", "nga", "claude", false);
    let mut r = HostRegistry::new();
    r.add_host(HostId::new("mini-1"), Transport::Local);
    let flaky_state = Arc::new(Mutex::new(false));
    struct Flaky(FakeTmux, Arc<Mutex<bool>>);
    impl TmuxRunner for Flaky {
        fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
            if *self.1.lock().unwrap() {
                return Err(TmuxError::Failed {
                    code: Some(1),
                    stderr: "no server running on /tmp/x".into(),
                });
            }
            self.0.run(args)
        }
    }
    let inner = FakeTmux {
        panes: Arc::new(Mutex::new(panes)),
        screens: sc.clone(),
        fail: None,
    };
    let runner: BoxedRunner = Box::new(Flaky(inner, flaky_state.clone()));
    r.add_server_with_runner(
        &HostId::new("mini-1"),
        ServerId::new("flight"),
        TmuxEndpoint::named("flight").unwrap(),
        runner,
    )
    .unwrap();
    let mut c = Collector::new(r);
    c.collect(1000); // Busy
    sc.lock().unwrap().insert("%1".into(), IDLE_SCREEN.into());
    assert_eq!(c.collect(1010).hosts[0].panes[0].state, Done);
    *flaky_state.lock().unwrap() = true;
    assert_eq!(c.collect(1020).hosts[0].health, HostHealth::NoServer);
    *flaky_state.lock().unwrap() = false;
    assert_eq!(
        c.collect(1030).hosts[0].panes[0].state,
        Done,
        "memory kept across the outage"
    );
}

#[test]
fn a_vanished_pane_is_forgotten_so_a_new_pane_with_its_id_starts_cold() {
    let sc = screens(&[("%1", BUSY_SCREEN)]);
    let table = Arc::new(Mutex::new(pane_row("%1", "nga", "claude", false)));
    let mut r = HostRegistry::new();
    r.add_host(HostId::new("mini-1"), Transport::Local);
    let runner: BoxedRunner = Box::new(FakeTmux {
        panes: table.clone(),
        screens: sc.clone(),
        fail: None,
    });
    r.add_server_with_runner(
        &HostId::new("mini-1"),
        ServerId::new("flight"),
        TmuxEndpoint::named("flight").unwrap(),
        runner,
    )
    .unwrap();
    let mut c = Collector::new(r);
    assert_eq!(c.collect(1000).hosts[0].panes[0].state, Busy);
    // The pane disappears from a host that answers: its memory is pruned.
    table.lock().unwrap().clear();
    assert!(c.collect(1010).hosts[0].panes.is_empty());
    // A new pane reuses the id, idle. With no memory of the old Busy it is Idle, not Done.
    *table.lock().unwrap() = pane_row("%1", "nga", "claude", false);
    sc.lock().unwrap().insert("%1".into(), IDLE_SCREEN.into());
    assert_eq!(c.collect(1020).hosts[0].panes[0].state, Idle);
}

#[test]
fn preview_captures_plain_text_of_the_selected_pane() {
    let sc = screens(&[("%1", PERMIT_SCREEN)]);
    let c = Collector::new(registry(pane_row("%1", "nga", "claude", false), &sc));
    let p = c.preview(&flight_ui_pane_ref());
    assert!(p
        .content
        .unwrap()
        .iter()
        .any(|l| l.contains("Do you want to proceed?")));
}

fn flight_ui_pane_ref() -> flight_state::PaneRef {
    flight_state::PaneRef {
        host: HostId::new("mini-1"),
        server: ServerId::new("flight"),
        pane: flight_state::PaneId::new("%1"),
    }
}
