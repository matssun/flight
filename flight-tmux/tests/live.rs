// SPDX-License-Identifier: MIT

//! Runs against a real tmux server on a private socket. Skipped when tmux is absent.
//! Never touches the default tmux server: every call carries `-S <unique private path>`.

use flight_tmux::{Tmux, TmuxEndpoint};
use std::process::Command;

struct Server {
    tmux: Tmux,
    socket: std::path::PathBuf,
}

impl Server {
    fn start() -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        // A private socket path (outside tmux's shared socket dir), removed on drop. The
        // thread id keeps parallel tests apart.
        let socket = std::env::temp_dir().join(format!(
            "flight-test-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        ));
        Some(Self {
            tmux: Tmux::new(TmuxEndpoint::Path(socket.clone())),
            socket,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
        let _ = std::fs::remove_file(&self.socket);
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

/// A service (launchd, systemd) or a non-login ssh command has no `LANG`/`LC_*`. tmux then
/// rewrites the tab separators in `-F` output to `_`, which once made a node report "online,
/// 0 panes". The argument list Flight really uses must survive an empty environment.
#[test]
fn pane_list_parses_with_no_locale_in_the_environment() {
    let Some(s) = Server::start() else { return };
    s.tmux.new_session("api", "/tmp").unwrap();
    let endpoint = TmuxEndpoint::Path(s.socket.clone());
    let path = std::env::var("PATH").unwrap_or_default();
    let run = |args: Vec<String>| {
        let out = Command::new("tmux")
            .env_clear()
            .env("PATH", &path)
            .args(args)
            .output()
            .expect("run tmux");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // Without -u, in this environment, the separators are rewritten (the original bug).
    let without_u: Vec<String> = endpoint
        .args()
        .into_iter()
        .chain(["list-panes", "-a", "-F", flight_tmux::PANE_FORMAT].map(str::to_owned))
        .collect();
    assert!(
        flight_tmux::parse_panes_output(&run(without_u)).is_empty(),
        "tmux no longer rewrites separators without -u: this guard can be relaxed"
    );

    // With the arguments Flight uses, the same environment yields the pane.
    let with_u = flight_tmux::tmux_args(
        &endpoint,
        &["list-panes", "-a", "-F", flight_tmux::PANE_FORMAT],
    );
    let panes = flight_tmux::parse_panes_output(&run(with_u));
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert_eq!(panes[0].session_name, "api");
}
