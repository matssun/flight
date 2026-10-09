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
        // The old process is gone before the new one starts: it held the saved file's lock.
        drop(std::mem::take(&mut self.servers));
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

fn reported(live: &Live) -> Vec<flight_proto::SavedWorkspace> {
    live.servers.saved_report()
}

#[test]
fn the_report_tells_running_stopped_and_every_kind_of_unavailable_root_apart() {
    use flight_proto::{SavedHealthCode as H, SavedRootCode as R};
    let Some(mut live) = Live::start("report") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let r = reported(&live);
    assert_eq!(r.len(), 1);
    assert_eq!(
        (r[0].health, r[0].root_state),
        (H::Running as i32, R::Verified as i32)
    );
    assert!(r[0].workspace_id.starts_with("w-") && r[0].config_key.starts_with("c-"));
    assert_eq!(r[0].root, dir);

    live.lose_tmux();
    live.restart_node();
    let r = reported(&live);
    assert_eq!(
        (r[0].health, r[0].root_state),
        (H::Stopped as i32, R::Verified as i32)
    );
    assert!(r[0].workspace_id.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();
    let r = reported(&live);
    assert_eq!(
        (r[0].health, r[0].root_state),
        (H::Blocked as i32, R::Missing as i32)
    );
    assert_eq!(
        r[0].name, "nga",
        "still listed with everything needed to act on it"
    );

    // The same path with another directory in it is a question for the user, not a match. A
    // directory deleted and made again is often given the same inode number, so this is told
    // apart by creation time, which not every filesystem records; without it there is nothing
    // to detect, and the rest of the test stands.
    std::fs::create_dir(&dir).unwrap();
    if std::fs::metadata(&dir).and_then(|m| m.created()).is_err() {
        eprintln!("no creation time on this filesystem: skipping the recreated-directory check");
        return;
    }
    // Creation times can be as coarse as the filesystem's clock: make sure they differ.
    let r = reported(&live);
    assert_eq!(
        (r[0].health, r[0].root_state),
        (H::Blocked as i32, R::Changed as i32)
    );
    assert!(!r[0].detail.is_empty());
    assert_eq!(live.sessions().len(), 0, "reporting starts nothing");
}

#[test]
fn reporting_with_an_unreadable_saved_file_is_empty_and_leaves_the_file() {
    let Some(mut live) = Live::start("report-corrupt") else {
        return;
    };
    let file = live.root.join("state").join("workspaces.toml");
    std::fs::write(&file, "garbage = [").unwrap();
    live.restart_node();
    assert!(reported(&live).is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "garbage = [");
}

mod actions {
    use super::*;
    use flight_node::{SavedAction, SavedActionRequest};
    use flight_proto::ErrorKindCode;

    fn act(live: &Live, key: &str, action: SavedAction) -> Result<(), flight_node::ControlError> {
        live.servers.saved_action(&SavedActionRequest {
            config_key: key.to_owned(),
            action,
        })
    }

    fn key(live: &Live) -> String {
        first_workspace(live).key.to_string()
    }

    #[test]
    fn remove_forgets_the_reference_and_nothing_else() {
        let Some(live) = Live::start("act-remove") else {
            return;
        };
        let dir = live.dir("nga");
        std::fs::write(Path::new(&dir).join("keep.txt"), "x").unwrap();
        live.workspace("nga", &dir);
        let k = key(&live);
        act(&live, &k, SavedAction::Remove).unwrap();
        assert!(live.saved().active().unwrap().workspaces.is_empty());
        assert_eq!(live.sessions(), vec!["nga"], "the process keeps running");
        assert!(
            Path::new(&dir).join("keep.txt").exists(),
            "the files are untouched"
        );
        // A second removal finds nothing to remove and says so.
        act(&live, &k, SavedAction::Remove).unwrap_err();
    }

    #[test]
    fn set_root_changes_where_it_points_and_creates_nothing() {
        let Some(mut live) = Live::start("act-setroot") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        live.lose_tmux();
        live.restart_node();
        let k = key(&live);
        let elsewhere = live
            .root
            .join("not-made-yet")
            .to_string_lossy()
            .into_owned();
        act(&live, &k, SavedAction::SetRoot(elsewhere.clone())).unwrap();
        let w = first_workspace(&live);
        assert_eq!(
            (w.root.path.as_str(), w.root.identity),
            (elsewhere.as_str(), None)
        );
        assert!(
            !Path::new(&elsewhere).exists(),
            "setting a root creates no directory"
        );
        let r = reported(&live);
        assert_eq!(r[0].root_state, flight_proto::SavedRootCode::Missing as i32);
        // And back to one that exists, which is then restorable.
        act(&live, &k, SavedAction::SetRoot(dir)).unwrap();
        act(&live, &k, SavedAction::Restore).unwrap();
        assert_eq!(live.sessions(), vec!["nga"]);
    }

    #[test]
    fn a_changed_root_is_accepted_only_on_request_and_only_if_it_is_there() {
        let Some(mut live) = Live::start("act-accept") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        live.lose_tmux();
        live.restart_node();
        let k = key(&live);
        // Gone: there is nothing to accept.
        std::fs::remove_dir_all(&dir).unwrap();
        let e = act(&live, &k, SavedAction::AcceptRoot).unwrap_err();
        assert_eq!(e.kind, ErrorKindCode::InvalidDirectory);
        // Another directory at the path: restore refuses until it is accepted.
        std::fs::create_dir(&dir).unwrap();
        if std::fs::metadata(&dir).and_then(|m| m.created()).is_err() {
            return; // identity cannot tell them apart on this filesystem
        }
        assert_eq!(
            act(&live, &k, SavedAction::Restore).unwrap_err().kind,
            ErrorKindCode::InvalidDirectory
        );
        assert!(live.sessions().is_empty());
        act(&live, &k, SavedAction::AcceptRoot).unwrap();
        act(&live, &k, SavedAction::Restore).unwrap();
        assert_eq!(live.sessions(), vec!["nga"]);
    }

    #[test]
    fn restore_starts_once_and_repeating_it_changes_nothing() {
        let Some(mut live) = Live::start("act-restore") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        live.lose_tmux();
        live.restart_node();
        let k = key(&live);
        for _ in 0..3 {
            act(&live, &k, SavedAction::Restore).unwrap();
        }
        assert_eq!(live.sessions(), vec!["nga"]);
        assert_eq!(live.windows(), 1);
        assert!(first_workspace(&live).last_workspace_id.is_some());
    }

    #[test]
    fn restore_refuses_a_missing_directory_and_never_makes_it() {
        let Some(mut live) = Live::start("act-missing") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        live.lose_tmux();
        std::fs::remove_dir_all(&dir).unwrap();
        live.restart_node();
        let k = key(&live);
        let e = act(&live, &k, SavedAction::Restore).unwrap_err();
        assert_eq!(e.kind, ErrorKindCode::InvalidDirectory);
        assert!(!Path::new(&dir).exists() && live.sessions().is_empty());
    }

    #[test]
    fn an_imported_workspace_needs_trust_before_it_starts_anything() {
        let Some(mut live) = Live::start("act-trust") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        live.lose_tmux();
        live.restart_node();
        let k = key(&live);
        // Make it look imported, as an import would leave it.
        let file = live.root.join("state").join("workspaces.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(
            &file,
            format!("{text}\n").replace("origin = \"local\"", "origin = \"imported\""),
        )
        .unwrap();
        live.restart_node();
        assert!(reported(&live)[0].imported);
        let e = act(&live, &k, SavedAction::Restore).unwrap_err();
        assert_eq!(e.kind, ErrorKindCode::NotAuthorized);
        assert!(live.sessions().is_empty());
        act(&live, &k, SavedAction::Trust).unwrap();
        act(&live, &k, SavedAction::Restore).unwrap();
        assert_eq!(live.sessions(), vec!["nga"]);
    }

    #[test]
    fn an_agent_saved_without_permission_prompts_is_not_started_by_a_restore() {
        let Some(mut live) = Live::start("act-skip") else {
            return;
        };
        let dir = live.dir("nga");
        live.servers
            .create_session(&SessionRequest {
                name: "nga".to_owned(),
                dir,
                program: Program::ClaudeSkipPermissions,
            })
            .unwrap();
        live.lose_tmux();
        live.restart_node();
        let e = act(&live, &key(&live), SavedAction::Restore).unwrap_err();
        assert_eq!(e.kind, ErrorKindCode::NotAuthorized);
        assert!(live.sessions().is_empty());
    }

    #[test]
    fn unknown_keys_and_unusable_files_are_refused_in_the_same_words_for_every_action() {
        let Some(mut live) = Live::start("act-unknown") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        for action in [
            SavedAction::Restore,
            SavedAction::AcceptRoot,
            SavedAction::Trust,
            SavedAction::SetRoot("/tmp".to_owned()),
            SavedAction::Remove,
        ] {
            let e = act(&live, "c-nope", action).unwrap_err();
            assert_eq!(e.kind, ErrorKindCode::UnknownWorkspace);
        }
        act(&live, &key(&live), SavedAction::Retry).unwrap();
        // With the file unusable, nothing is changed and the user is told why.
        let file = live.root.join("state").join("workspaces.toml");
        std::fs::write(&file, "broken = [").unwrap();
        live.restart_node();
        let e = act(&live, "c-any", SavedAction::Remove).unwrap_err();
        assert_eq!(e.kind, ErrorKindCode::Unsupported);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "broken = [");
    }

    #[test]
    fn any_action_brings_the_next_report_forward() {
        let Some(live) = Live::start("act-retry") else {
            return;
        };
        let dir = live.dir("nga");
        live.workspace("nga", &dir);
        let before = live.servers.retries();
        act(&live, &key(&live), SavedAction::Retry).unwrap();
        assert!(live.servers.retries() > before);
    }
}

#[test]
fn a_second_node_on_the_same_saved_file_runs_without_persistence_and_leaves_the_first_alone() {
    let Some(live) = Live::start("lock") else {
        return;
    };
    let dir = live.dir("nga");
    live.workspace("nga", &dir);
    let second = WorkspacePersistence::open(&live.root.join("state"), HOST, None);
    assert!(second
        .disabled_reason()
        .unwrap()
        .contains("another Flight process"));
    assert!(second.document().is_none());
    // The first still owns the file and keeps working.
    assert!(live
        .servers
        .persistence()
        .unwrap()
        .disabled_reason()
        .is_none());
    assert_eq!(live.saved().active().unwrap().workspaces.len(), 1);
}
