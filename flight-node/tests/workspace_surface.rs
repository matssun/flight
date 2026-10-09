// SPDX-License-Identifier: MIT

//! Workspaces and their surfaces against a real tmux server on a private socket (skipped
//! without tmux). A workspace is made by `create_session`, gets a companion shell from
//! `create_surface`, and is found again from what the backend recorded: nothing is kept in
//! the node. A fake `claude` stands in for the real one, and no default tmux server is used.

use flight_node::{
    fresh_incarnation, Control, ControlError, NewSurface, NodeCore, Program, ServerOutcome,
    SessionEnv, SessionRequest, SurfaceRequest, TerminalSpec, TmuxServers,
};
use flight_proto::{ErrorKindCode, PaneState, SurfaceKindCode};
use flight_state::{HostId, PaneId, ServerId, WorkspaceId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const HOST: &str = "node-under-test";

struct Live {
    tmux: Tmux,
    endpoint: TmuxEndpoint,
    servers: TmuxServers,
    root: PathBuf,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let endpoint =
            TmuxEndpoint::named(&format!("flight-test-{}-ws-{tag}", std::process::id())).ok()?;
        let root =
            std::env::temp_dir().join(format!("flight-node-ws-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).ok()?;
        let root = root.canonicalize().ok()?;
        let claude = root.join("bin").join("claude");
        std::fs::write(&claude, "#!/bin/sh\nexec sleep 3600\n").ok()?;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).ok()?;
        let mut live = Self {
            tmux: Tmux::new(endpoint.clone()),
            servers: Self::servers(&endpoint, &root),
            endpoint,
            root,
        };
        live.restart_node();
        Some(live)
    }

    fn servers(endpoint: &TmuxEndpoint, root: &Path) -> TmuxServers {
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        servers.allow_terminal(ServerId::new("live"), endpoint.clone());
        servers.set_session_env(SessionEnv {
            home: Some(root.to_path_buf()),
            path: std::env::join_paths([root.join("bin"), "/usr/bin".into(), "/bin".into()])
                .unwrap(),
        });
        servers
    }

    /// A node that starts again: new in-memory state over the same backend.
    fn restart_node(&mut self) {
        self.servers = Self::servers(&self.endpoint, &self.root);
    }

    fn dir(&self, name: &str) -> String {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn workspace(&self, name: &str, dir: &str) {
        self.servers
            .create_session(&SessionRequest {
                name: name.to_owned(),
                dir: dir.to_owned(),
                program: Program::Claude,
            })
            .unwrap();
    }

    fn add_shell(&self, id: &str) -> Result<(), ControlError> {
        self.servers.create_surface(&SurfaceRequest {
            host: HostId::new(HOST),
            workspace_id: WorkspaceId::new(id),
            kind: NewSurface::Shell,
        })
    }

    /// What this node publishes now: a fresh node core fed one observation round.
    fn published(&self) -> Vec<PaneState> {
        let mut core = NodeCore::new(HostId::new(HOST), fresh_incarnation().unwrap());
        for round in self.servers.observe(1) {
            assert!(matches!(round.outcome, ServerOutcome::Observed(_)));
            core.apply(round);
        }
        let mut panes = core.state().panes;
        panes.sort_by(|a, b| a.surface_id.cmp(&b.surface_id));
        panes
    }

    fn windows(&self) -> usize {
        self.tmux.list_panes().map(|p| p.len()).unwrap_or(0)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn canonical(path: &str) -> PathBuf {
    Path::new(path).canonicalize().unwrap()
}

fn kind(p: &PaneState) -> SurfaceKindCode {
    SurfaceKindCode::try_from(p.surface_kind).unwrap()
}

#[test]
fn a_new_workspace_publishes_an_identity_that_is_neither_its_name_nor_a_backend_id() {
    let Some(live) = Live::start("identity") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let panes = live.published();
    assert_eq!(panes.len(), 1);
    let p = &panes[0];
    assert_eq!(kind(p), SurfaceKindCode::Agent);
    assert!(flight_state::valid_id(&p.workspace_id) && p.workspace_id.starts_with("w-"));
    assert!(flight_state::valid_id(&p.surface_id) && p.surface_id.starts_with("s-"));
    assert_ne!(p.workspace_id, "nga", "the name is not the identity");
    assert!(!p.workspace_id.contains('$') && !p.surface_id.contains('%'));
    assert_eq!(canonical(&p.workspace_root), canonical(&dir));
    assert_eq!(p.session, "nga");
}

#[test]
fn two_workspaces_with_the_same_directory_are_two_workspaces() {
    let Some(live) = Live::start("two") else {
        return;
    };
    let dir = live.dir("shared");
    live.workspace("one", &dir);
    live.workspace("two", &dir);
    let panes = live.published();
    assert_eq!(panes.len(), 2);
    assert_ne!(panes[0].workspace_id, panes[1].workspace_id);
}

#[test]
fn the_identity_is_rediscovered_after_the_node_starts_again() {
    let Some(mut live) = Live::start("restart") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    live.add_shell(&live.published()[0].workspace_id).unwrap();
    let before: Vec<(String, String, i32)> = live
        .published()
        .iter()
        .map(|p| (p.workspace_id.clone(), p.surface_id.clone(), p.surface_kind))
        .collect();
    assert_eq!(before.len(), 2);
    live.restart_node();
    let after: Vec<(String, String, i32)> = live
        .published()
        .iter()
        .map(|p| (p.workspace_id.clone(), p.surface_id.clone(), p.surface_kind))
        .collect();
    assert_eq!(before, after, "the same workspace and surfaces, by id");
}

#[test]
fn the_companion_shell_inherits_the_workspace_and_its_root() {
    let Some(live) = Live::start("shell") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let id = live.published()[0].workspace_id.clone();
    live.add_shell(&id).unwrap();

    let panes = live.published();
    assert_eq!(panes.len(), 2);
    let agent = panes.iter().find(|p| kind(p) == SurfaceKindCode::Agent);
    let shell = panes.iter().find(|p| kind(p) == SurfaceKindCode::Shell);
    let (agent, shell) = (agent.unwrap(), shell.unwrap());
    assert_eq!(agent.workspace_id, id);
    assert_eq!(shell.workspace_id, id, "the same workspace");
    assert_ne!(agent.surface_id, shell.surface_id);
    assert_eq!(
        canonical(&shell.path),
        canonical(&dir),
        "starts in the root"
    );
    assert_eq!(canonical(&shell.workspace_root), canonical(&dir));
    assert_eq!(shell.session, "nga");
}

#[test]
fn a_second_shell_is_refused_and_adds_nothing() {
    let Some(live) = Live::start("dup") else {
        return;
    };
    live.workspace("nga", &live.dir("nga"));
    let id = live.published()[0].workspace_id.clone();
    live.add_shell(&id).unwrap();
    assert_eq!(live.windows(), 2);
    let err = live.add_shell(&id).unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::AlreadyExists);
    assert_eq!(live.windows(), 2, "no second shell");
}

#[test]
fn a_workspace_this_node_does_not_have_is_refused_and_adds_nothing() {
    let Some(live) = Live::start("unknown") else {
        return;
    };
    live.workspace("nga", &live.dir("nga"));
    let err = live.add_shell("w-0000000000000000").unwrap_err();
    assert_eq!(err.kind, ErrorKindCode::UnknownWorkspace);
    assert_eq!(live.windows(), 1);
}

#[test]
fn only_the_workspaces_own_session_gains_the_shell() {
    let Some(live) = Live::start("others") else {
        return;
    };
    live.workspace("one", &live.dir("one"));
    live.workspace("two", &live.dir("two"));
    // A session of the user's own, which Flight did not make.
    live.tmux.new_session("mine", "/tmp").unwrap();
    let one = live
        .published()
        .into_iter()
        .find(|p| p.session == "one")
        .unwrap();
    live.add_shell(&one.workspace_id).unwrap();
    let count = |session: &str| {
        live.tmux
            .list_panes()
            .unwrap()
            .iter()
            .filter(|p| p.session_name == session)
            .count()
    };
    assert_eq!((count("one"), count("two"), count("mine")), (2, 1, 1));
}

#[test]
fn a_session_that_predates_workspaces_is_a_workspace_of_its_agent_and_can_get_a_shell() {
    let Some(live) = Live::start("legacy") else {
        return;
    };
    // Made by hand, with no Flight markers at all: a Flight session by the old option only.
    let dir = live.dir("old");
    // A program that the node recognises by name as Claude, and that just waits.
    let claude = live.root.join("legacy-claude");
    std::fs::create_dir_all(&claude).unwrap();
    let claude = claude.join("claude");
    let cat = String::from_utf8(Command::new("which").arg("cat").output().unwrap().stdout).unwrap();
    // A copy, not a symlink: tmux reports the resolved program's name. On macOS a copied
    // system binary only runs once it is signed again.
    std::fs::copy(cat.trim(), &claude).unwrap();
    if cfg!(target_os = "macos") {
        Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(&claude)
            .output()
            .unwrap();
    }
    let out = live
        .tmux
        .runner()
        .run(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{session_id}",
            "-s",
            "old",
            "-c",
            &dir,
            claude.to_str().unwrap(),
        ])
        .unwrap();
    live.tmux
        .runner()
        .run(&[
            "set-option",
            "-t",
            out.stdout.trim(),
            "@flight_session",
            "1",
        ])
        .unwrap();
    let panes = live.published();
    assert_eq!(panes.len(), 1);
    assert_eq!(kind(&panes[0]), SurfaceKindCode::Agent);
    assert!(panes[0].workspace_id.starts_with("legacy."));
    assert_eq!(canonical(&panes[0].workspace_root), canonical(&dir));
    live.add_shell(&panes[0].workspace_id).unwrap();
    let panes = live.published();
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0].workspace_id, panes[1].workspace_id);
}

fn open(live: &Live, p: &PaneState) -> flight_node::OpenedTerminal {
    let pane = p.pane_ref.as_ref().unwrap();
    live.servers
        .open_terminal(&TerminalSpec {
            request_id: 1,
            // Every terminal has an id of its own, and so has its view.
            terminal_id: [u8::try_from(p.pid % 251).unwrap_or(7); 16],
            server: ServerId::new("live"),
            pane: PaneId::new(&pane.pane),
            pid: p.pid,
            cols: 90,
            rows: 25,
            term: "xterm-256color".to_owned(),
        })
        .unwrap()
}

#[test]
fn each_surface_is_attachable_alone_and_leaving_one_leaves_the_other_running() {
    let Some(live) = Live::start("attach") else {
        return;
    };
    live.workspace("nga", &live.dir("nga"));
    live.add_shell(&live.published()[0].workspace_id).unwrap();
    let panes = live.published();
    let agent = panes
        .iter()
        .find(|p| kind(p) == SurfaceKindCode::Agent)
        .unwrap();
    let shell = panes
        .iter()
        .find(|p| kind(p) == SurfaceKindCode::Shell)
        .unwrap();

    let mut on_shell = open(&live, shell);
    let mut on_agent = open(&live, agent);
    // Both presentations are on at once, one client each.
    let clients = || {
        live.tmux
            .runner()
            .run(&["list-clients", "-F", "#{client_session}"])
            .map(|o| o.stdout.lines().count())
            .unwrap_or(0)
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while clients() < 2 {
        assert!(std::time::Instant::now() < deadline, "two clients");
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    // Leaving the shell's presentation does not touch the agent, nor the shell itself.
    assert!(on_shell
        .process
        .hang_up(std::time::Duration::from_secs(5))
        .is_some());
    assert_eq!(live.windows(), 2, "both surfaces are still there");
    let after = live.published();
    assert_eq!(after.len(), 2);
    assert!(on_agent
        .process
        .hang_up(std::time::Duration::from_secs(5))
        .is_some());
    assert_eq!(live.windows(), 2);
}

#[test]
fn the_default_tmux_server_is_never_used() {
    let Some(live) = Live::start("isolated") else {
        return;
    };
    live.workspace("nga", &live.dir("nga"));
    live.add_shell(&live.published()[0].workspace_id).unwrap();
    // The private server is the only one with these sessions; nothing in the default one
    // carries Flight's markers.
    let default = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{@flight_workspace}"])
        .env_remove("TMUX")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&default.stdout);
    assert!(
        !text.contains("w-"),
        "a workspace appeared in the default server: {text}"
    );
}
