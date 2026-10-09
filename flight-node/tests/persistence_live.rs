// SPDX-License-Identifier: MIT

//! Durable workspaces against a real tmux server on a private socket (skipped without tmux):
//! what a node saves when a workspace is created, and what recovery does and refuses to do
//! after the tmux server, the node or the directory is lost (ADR-008).

use flight_node::{
    Control, NewSurface, Program, SessionEnv, SessionRequest, SurfaceRequest, TmuxServers,
    WorkspacePersistence,
};
use flight_state::{HostId, ServerId, WorkspaceId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint};
use flight_workspaces::{Blocker, Health, RecoveryPolicy, RecoveryReport, RootCheck};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const HOST: &str = "node-under-test";

struct Live {
    tmux: Tmux,
    socket: String,
    endpoint: TmuxEndpoint,
    servers: Arc<TmuxServers>,
    root: PathBuf,
}

fn go() -> RecoveryPolicy {
    RecoveryPolicy {
        start_missing: true,
        ..RecoveryPolicy::default()
    }
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let socket = format!("flight-test-{}-pl-{tag}", std::process::id());
        let endpoint = TmuxEndpoint::named(&socket).ok()?;
        let root =
            std::env::temp_dir().join(format!("flight-node-pl-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).ok()?;
        std::fs::create_dir_all(root.join("state")).ok()?;
        let root = root.canonicalize().ok()?;
        let claude = root.join("bin").join("claude");
        std::fs::write(&claude, "#!/bin/sh\nexec sleep 3600\n").ok()?;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).ok()?;
        let servers = Self::node(&endpoint, &root);
        Some(Self {
            tmux: Tmux::new(endpoint.clone()),
            socket,
            endpoint,
            servers,
            root,
        })
    }

    fn node(endpoint: &TmuxEndpoint, root: &Path) -> Arc<TmuxServers> {
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        servers.set_session_env(SessionEnv {
            home: Some(root.to_path_buf()),
            path: std::env::join_paths([root.join("bin"), "/usr/bin".into(), "/bin".into()])
                .unwrap(),
        });
        servers.enable_persistence(WorkspacePersistence::open(
            &root.join("state"),
            HOST,
            Some(root.to_path_buf()),
        ));
        Arc::new(servers)
    }

    /// The node process starts again: nothing in memory survives, the saved file does.
    fn restart_node(&mut self) {
        self.servers = Self::node(&self.endpoint, &self.root);
    }

    fn lose_tmux(&self) {
        let _ = self.tmux.kill_server();
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

    fn saved(&self) -> flight_workspaces::Document {
        self.servers.persistence().unwrap().document().unwrap()
    }

    fn recover(&self, policy: &RecoveryPolicy) -> RecoveryReport {
        self.servers
            .persistence()
            .unwrap()
            .recover(&self.servers, policy)
            .unwrap()
            .unwrap()
    }

    fn sessions(&self) -> Vec<String> {
        let out = Command::new("tmux")
            .args(["-L", &self.socket, "list-sessions", "-F", "#{session_name}"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
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

fn first_workspace(live: &Live) -> flight_workspaces::WorkspaceDefinition {
    live.saved().active().unwrap().workspaces[0].clone()
}

#[test]
fn creating_a_workspace_saves_it_and_adding_a_shell_updates_it() {
    let Some(live) = Live::start("save") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let w = first_workspace(&live);
    assert_eq!(
        (w.name.as_str(), w.root.path.as_str(), w.surfaces.len()),
        ("nga", dir.as_str(), 1)
    );
    assert!(
        w.root.identity.is_some(),
        "the directory was recorded when it was seen"
    );
    let id = w.last_workspace_id.clone().unwrap();
    live.servers
        .create_surface(&SurfaceRequest {
            host: HostId::new(HOST),
            workspace_id: WorkspaceId::new(&id),
            kind: NewSurface::Shell,
        })
        .unwrap();
    assert_eq!(first_workspace(&live).surfaces.len(), 2);
    // A node that starts again reads the same thing back.
    let mut live = live;
    live.restart_node();
    assert_eq!(
        live.saved().active().unwrap().workspaces[0].surfaces.len(),
        2
    );
}

#[test]
fn after_the_tmux_server_is_lost_the_workspace_stays_saved_and_nothing_starts_by_itself() {
    let Some(mut live) = Live::start("lost") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    live.lose_tmux();
    live.restart_node();
    let report = live.recover(&RecoveryPolicy::default());
    assert_eq!(report.items[0].health, Health::Stopped);
    assert!(report.done.is_empty());
    assert!(
        live.sessions().is_empty(),
        "recovery without permission starts nothing"
    );
    assert_eq!(live.saved().active().unwrap().workspaces.len(), 1);
}

#[test]
fn restoring_replaces_the_lost_workspace_once_however_often_it_is_repeated() {
    let Some(mut live) = Live::start("restore") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let old = first_workspace(&live).last_workspace_id.unwrap();
    live.lose_tmux();
    live.restart_node();
    for _ in 0..3 {
        live.recover(&go());
    }
    assert_eq!(live.sessions(), vec!["nga"]);
    assert_eq!(live.windows(), 1);
    let new = first_workspace(&live).last_workspace_id.unwrap();
    assert_ne!(
        old, new,
        "a replacement process has a runtime identity of its own"
    );
    // The key written into tmux is the saved one, so the next node finds it again.
    live.restart_node();
    let report = live.recover(&go());
    assert_eq!(report.items[0].health, Health::Running);
    assert!(report.done.is_empty());
    assert_eq!(live.windows(), 1);
}

#[test]
fn two_passes_at_the_same_time_start_one_workspace() {
    let Some(mut live) = Live::start("race") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    live.lose_tmux();
    live.restart_node();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let servers = live.servers.clone();
            std::thread::spawn(move || {
                servers
                    .persistence()
                    .unwrap()
                    .recover(&servers, &go())
                    .unwrap()
                    .unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(live.sessions(), vec!["nga"]);
}

#[test]
fn a_start_whose_reply_was_lost_is_found_by_its_mark_and_not_repeated() {
    let Some(mut live) = Live::start("mark") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let w = first_workspace(&live);
    let key = w.key.to_string();
    live.lose_tmux();
    live.restart_node();
    // The workspace is started as recovery would, and the node dies before it hears back: the
    // session exists, marked with the saved key, under a runtime identity nobody recorded.
    let tmux = |args: &[&str]| {
        let mut full = vec!["-L", live.socket.as_str()];
        full.extend_from_slice(args);
        assert!(Command::new("tmux").args(full).status().unwrap().success());
    };
    tmux(&["new-session", "-d", "-s", "nga", "-c", &dir, "sleep 3600"]);
    tmux(&["set-option", "-t", "nga", "@flight_session", "1"]);
    tmux(&[
        "set-option",
        "-t",
        "nga",
        "@flight_workspace",
        "w-unrecorded",
    ]);
    tmux(&["set-option", "-t", "nga", "@flight_config", &key]);
    let report = live.recover(&go());
    assert_eq!(
        report.items[0].health,
        Health::Partial,
        "the agent is there; the saved set has only it"
    );
    assert_eq!(live.sessions(), vec!["nga"]);
    assert_eq!(
        first_workspace(&live).last_workspace_id.as_deref(),
        Some("w-unrecorded")
    );
}

#[test]
fn a_missing_directory_is_never_created_and_the_saved_workspace_waits() {
    let Some(mut live) = Live::start("missing") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let before = live.saved();
    live.lose_tmux();
    std::fs::remove_dir_all(&dir).unwrap();
    live.restart_node();
    for _ in 0..3 {
        let report = live.recover(&go());
        assert!(matches!(
            report.items[0].health,
            Health::Blocked(Blocker::Root(_))
        ));
        assert!(report.done.is_empty());
    }
    assert!(live.sessions().is_empty());
    assert!(
        !Path::new(&dir).exists(),
        "recovery does not recreate the directory"
    );
    assert_eq!(live.saved(), before, "failed attempts change nothing saved");
}

#[test]
fn a_different_directory_at_the_same_path_does_not_inherit_the_workspace() {
    let Some(mut live) = Live::start("changed") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    live.lose_tmux();
    let moved = format!("{dir}.original");
    std::fs::rename(&dir, &moved).unwrap();
    std::fs::create_dir(&dir).unwrap(); // someone else's directory, same path
    live.restart_node();
    let report = live.recover(&go());
    assert!(
        matches!(report.items[0].root, RootCheck::Changed(_)),
        "{:?}",
        report.items[0].root
    );
    assert!(live.sessions().is_empty());
    // The original comes back to its place: now it is the saved one again.
    std::fs::remove_dir(&dir).unwrap();
    std::fs::rename(&moved, &dir).unwrap();
    live.recover(&go());
    assert_eq!(live.sessions(), vec!["nga"]);
}

#[test]
fn an_unreadable_saved_file_is_kept_byte_for_byte_and_creation_still_works() {
    let Some(mut live) = Live::start("corrupt") else {
        return;
    };
    let file = live.root.join("state").join("workspaces.toml");
    let junk = "schema_version = [not toml";
    std::fs::write(&file, junk).unwrap();
    live.restart_node();
    assert!(live
        .servers
        .persistence()
        .unwrap()
        .disabled_reason()
        .is_some());
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    assert_eq!(live.sessions(), vec!["nga"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), junk);
    assert!(live
        .servers
        .persistence()
        .unwrap()
        .recover(&live.servers, &go())
        .is_none());
}

#[test]
fn a_file_from_a_newer_flight_is_not_overwritten() {
    let Some(mut live) = Live::start("newer") else {
        return;
    };
    let file = live.root.join("state").join("workspaces.toml");
    let newer = "schema_version = 99\nactive_profile = \"default\"\n";
    std::fs::write(&file, newer).unwrap();
    live.restart_node();
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
}

#[test]
fn a_workspace_made_by_hand_is_recorded_once_and_a_removed_one_stays_removed() {
    let Some(mut live) = Live::start("adopt") else {
        return;
    };
    let dir = live.dir("hand");
    // Not made through Flight's create: an unmarked session running a known agent's program.
    assert!(Command::new("tmux")
        .args([
            "-L",
            &live.socket,
            "new-session",
            "-d",
            "-s",
            "hand",
            "-c",
            &dir,
            "sleep 3600"
        ])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("tmux")
        .args([
            "-L",
            &live.socket,
            "set-option",
            "-t",
            "hand",
            "@flight_session",
            "1"
        ])
        .status()
        .unwrap()
        .success());
    let first = live.recover(&RecoveryPolicy::default());
    assert_eq!(first.recorded.len(), 1);
    assert!(live.recover(&RecoveryPolicy::default()).recorded.is_empty());
    live.restart_node();
    assert_eq!(live.saved().active().unwrap().workspaces.len(), 1);
    let key = first_workspace(&live).key;
    live.servers.persistence().unwrap().remove(&key).unwrap();
    live.recover(&RecoveryPolicy::default());
    assert!(live.saved().active().unwrap().workspaces.is_empty());
    assert_eq!(
        live.sessions(),
        vec!["hand"],
        "removing the reference leaves the session running"
    );
}
