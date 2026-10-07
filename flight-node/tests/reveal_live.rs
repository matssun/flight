// SPDX-License-Identifier: MIT

//! `Control::reveal_pane` against a real tmux server on a private socket (skipped without
//! tmux). A reveal changes what a session shows, never which session a client is attached to,
//! and it acts only on the pane process the caller named.

use flight_node::{Control, TmuxServers};
use flight_proto::ErrorKindCode;
use flight_state::{PaneId, ServerId};
use flight_tmux::{ControlConnection, SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use std::process::Command;

struct Live {
    endpoint: TmuxEndpoint,
    tmux: Tmux,
    servers: TmuxServers,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let endpoint =
            TmuxEndpoint::named(&format!("flight-test-{}-{tag}", std::process::id())).ok()?;
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        Some(Self {
            tmux: Tmux::new(endpoint.clone()),
            endpoint,
            servers,
        })
    }

    fn raw(&self, args: &[&str]) -> String {
        self.tmux.runner().run(args).unwrap().stdout
    }

    /// `window_index:pane_id` of what the session currently shows.
    fn showing(&self, session: &str) -> String {
        self.raw(&[
            "display-message",
            "-p",
            "-t",
            &format!("={session}:"),
            "#{window_index}:#{pane_id}",
        ])
        .trim()
        .to_owned()
    }

    fn pid_of(&self, pane: &str) -> u32 {
        self.raw(&["display-message", "-p", "-t", pane, "#{pane_pid}"])
            .trim()
            .parse()
            .unwrap()
    }

    fn reveal(&self, pane: &str, pid: u32) -> Result<(), flight_node::ControlError> {
        self.servers
            .reveal_pane(&ServerId::new("live"), &PaneId::new(pane), pid)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
    }
}

/// Session `a`: window 0 with two panes (the first active), then window 1 which is current.
fn two_windows(live: &Live) -> (String, String, String) {
    live.tmux
        .new_session_running("a", "/tmp", "sleep 600")
        .unwrap();
    live.raw(&["split-window", "-t", "=a:0", "sleep 600"]);
    live.raw(&["select-pane", "-t", "=a:0.0"]);
    live.raw(&["new-window", "-t", "=a:", "sleep 600"]);
    let panes = live.raw(&[
        "list-panes",
        "-a",
        "-F",
        "#{window_index}.#{pane_index} #{pane_id}",
    ]);
    let id = |wp: &str| {
        panes
            .lines()
            .find_map(|l| l.strip_prefix(wp).map(|r| r.trim().to_owned()))
            .unwrap_or_else(|| panic!("no pane {wp} in {panes}"))
    };
    (id("0.0"), id("0.1"), id("1.0"))
}

#[test]
fn reveal_selects_the_window_and_the_pane() {
    let Some(live) = Live::start("select") else {
        return;
    };
    let (first, second, third) = two_windows(&live);
    assert_eq!(
        live.showing("a"),
        format!("1:{third}"),
        "window 1 is current"
    );

    live.reveal(&second, live.pid_of(&second)).unwrap();
    assert_eq!(live.showing("a"), format!("0:{second}"));

    live.reveal(&first, live.pid_of(&first)).unwrap();
    assert_eq!(live.showing("a"), format!("0:{first}"));
}

#[test]
fn reveal_does_not_move_any_client_between_sessions() {
    let Some(live) = Live::start("client") else {
        return;
    };
    live.tmux
        .new_session_running("a", "/tmp", "sleep 600")
        .unwrap();
    live.tmux
        .new_session_running("b", "/tmp", "sleep 600")
        .unwrap();
    live.raw(&["new-window", "-t", "=a:", "sleep 600"]);
    let target = live
        .raw(&["list-panes", "-t", "=a:0", "-F", "#{pane_id}"])
        .trim()
        .to_owned();

    // A client attached to `b` (a control client is a client).
    let mut watcher = ControlConnection::open(&live.endpoint).unwrap();
    let attached = |w: &mut ControlConnection| {
        let r = w
            .run(&["list-clients -F '#{client_pid} #{client_session}'".to_owned()])
            .unwrap();
        r[0].lines.clone()
    };
    let before = attached(&mut watcher);
    live.reveal(&target, live.pid_of(&target)).unwrap();
    assert_eq!(
        attached(&mut watcher),
        before,
        "a reveal must not touch clients"
    );
    assert_eq!(live.showing("a"), format!("0:{target}"));
}

#[test]
fn a_replaced_pane_is_refused_and_nothing_changes() {
    let Some(live) = Live::start("replaced") else {
        return;
    };
    let (_first, second, third) = two_windows(&live);
    let seen = live.pid_of(&second);

    // Same pane id, new process.
    live.raw(&["respawn-pane", "-k", "-t", &second, "sleep 601"]);
    assert_ne!(live.pid_of(&second), seen);

    let err = live.reveal(&second, seen).unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::PaneChanged);
    assert_eq!(
        live.showing("a"),
        format!("1:{third}"),
        "a refused reveal changes nothing"
    );
}

#[test]
fn a_pane_id_reused_by_a_new_server_is_not_the_pane_the_caller_saw() {
    let Some(live) = Live::start("reuse") else {
        return;
    };
    live.tmux
        .new_session_running("a", "/tmp", "sleep 600")
        .unwrap();
    let id = live
        .raw(&["list-panes", "-a", "-F", "#{pane_id}"])
        .trim()
        .to_owned();
    let seen = live.pid_of(&id);

    live.tmux.kill_server().unwrap();
    live.tmux
        .new_session_running("a", "/tmp", "sleep 602")
        .unwrap();
    assert_eq!(
        live.raw(&["list-panes", "-a", "-F", "#{pane_id}"]).trim(),
        id,
        "tmux reused the id"
    );

    let err = live.reveal(&id, seen).unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::PaneChanged);
}

#[test]
fn unknown_panes_and_missing_servers_are_typed_errors() {
    let Some(live) = Live::start("typed") else {
        return;
    };
    let err = live.reveal("%1", 1).unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::TmuxServerUnavailable, "{err:?}");

    live.tmux
        .new_session_running("a", "/tmp", "sleep 600")
        .unwrap();
    let err = live.reveal("%99", 1).unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::UnknownPane, "{err:?}");
}
