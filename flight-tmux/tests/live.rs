// SPDX-License-Identifier: MIT

//! Runs against a real tmux server on a private socket. Skipped when tmux is absent.
//! Never touches the default tmux server: every call carries `-L <unique name>`.

use flight_tmux::{Tmux, TmuxEndpoint};
use std::process::Command;

struct Server {
    tmux: Tmux,
}

impl Server {
    fn start() -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let name = format!("flight-test-{}", std::process::id());
        Some(Self {
            tmux: Tmux::new(TmuxEndpoint::named(&name).ok()?),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
    }
}

#[test]
fn session_lifecycle_on_private_socket() {
    let Some(s) = Server::start() else { return };
    let t = &s.tmux;
    assert!(!t.has_session("api"));
    t.new_session("api", "/tmp").unwrap();
    assert!(t.has_session("api"));

    let panes = t.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].session_name, "api");
    assert!(panes[0].pane_id.starts_with('%'));

    t.capture_pane(&panes[0].pane_id, false, Some(5)).unwrap();
    t.kill_session("api").unwrap();
    assert!(!t.has_session("api"));
}

#[test]
fn private_socket_is_isolated_from_other_servers() {
    let Some(s) = Server::start() else { return };
    // No session was created, so there is no server: listing must fail, not fall back
    // to whatever default server the developer happens to be running.
    assert!(s.tmux.list_panes().is_err());
}
