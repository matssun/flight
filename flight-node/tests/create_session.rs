// SPDX-License-Identifier: MIT

//! `Control::create_session` against a real tmux server on a private socket (skipped without
//! tmux). The node validates, resolves and launches; every refusal leaves nothing behind and
//! no existing session is touched. A fake `claude` stands in for the real one.

use flight_node::{
    Control, ControlError, Program, ServerOutcome, SessionEnv, SessionRequest, TmuxServers,
};
use flight_proto::ErrorKindCode;
use flight_state::ServerId;
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Live {
    tmux: Tmux,
    servers: TmuxServers,
    root: PathBuf,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let endpoint =
            TmuxEndpoint::named(&format!("flight-test-{}-create-{tag}", std::process::id()))
                .ok()?;
        let root =
            std::env::temp_dir().join(format!("flight-node-create-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).ok()?;
        let root = root.canonicalize().ok()?;
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        let mut live = Self {
            tmux: Tmux::new(endpoint),
            servers,
            root,
        };
        live.use_path(&[live.root.join("bin")]);
        Some(live)
    }

    /// The node's `PATH` becomes exactly these directories (plus the system's, for `sh`).
    fn use_path(&mut self, dirs: &[PathBuf]) {
        let mut all: Vec<PathBuf> = dirs.to_vec();
        all.extend(["/usr/bin", "/bin"].map(PathBuf::from));
        self.servers.set_session_env(SessionEnv {
            home: Some(self.root.clone()),
            path: std::env::join_paths(all).unwrap(),
        });
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.root.join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn dir(&self, name: &str) -> String {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn create(&self, name: &str, dir: &str, program: Program) -> Result<(), ControlError> {
        self.servers.create_session(&SessionRequest {
            server: ServerId::new("live"),
            name: name.to_owned(),
            dir: dir.to_owned(),
            program,
        })
    }

    fn sessions(&self) -> Vec<String> {
        self.tmux
            .list_panes()
            .map(|p| p.into_iter().map(|p| p.session_name).collect())
            .unwrap_or_default()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn kind(e: ControlError) -> ErrorKindCode {
    e.kind
}

fn canonical(path: &str) -> PathBuf {
    Path::new(path).canonicalize().unwrap()
}

#[test]
fn a_shell_session_starts_in_the_directory_and_is_published() {
    let Some(live) = Live::start("shell") else {
        return;
    };
    let dir = live.dir("proj");
    live.create("work", &dir, Program::Shell).unwrap();

    let panes = live.tmux.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].session_name, "work");
    assert_eq!(canonical(&panes[0].current_path), canonical(&dir));

    // It shows up in the node's next observation although it runs no agent.
    let rounds = live.servers.observe(1);
    let ServerOutcome::Observed(observed) = &rounds[0].outcome else {
        panic!("{rounds:?}");
    };
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].session, "work");
}

#[test]
fn a_session_flight_did_not_create_stays_unpublished_when_it_runs_no_agent() {
    let Some(live) = Live::start("foreign") else {
        return;
    };
    live.tmux.new_session("mine", "/tmp").unwrap();
    live.create("work", &live.dir("p"), Program::Shell).unwrap();
    let rounds = live.servers.observe(1);
    let ServerOutcome::Observed(observed) = &rounds[0].outcome else {
        panic!("{rounds:?}");
    };
    let names: Vec<&str> = observed.iter().map(|p| p.session.as_str()).collect();
    assert_eq!(names, ["work"]);
}

#[test]
fn claude_found_on_the_nodes_path_runs_in_the_requested_directory() {
    let Some(live) = Live::start("claude") else {
        return;
    };
    let dir = live.dir("proj");
    let out = live.root.join("out");
    live.script("claude", &format!("pwd > {0}; sleep 60", out.display()));
    live.create("agent", &dir, Program::Claude).unwrap();

    for _ in 0..50 {
        if std::fs::read_to_string(&out).is_ok_and(|s| !s.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(canonical(written.trim()), canonical(&dir));
    assert_eq!(live.sessions(), ["agent"]);
}

#[test]
fn an_existing_name_is_refused_and_the_session_is_untouched() {
    let Some(live) = Live::start("exists") else {
        return;
    };
    live.tmux.new_session("work", "/tmp").unwrap();
    let before = live.tmux.list_panes().unwrap();
    let err = live
        .create("work", &live.dir("p"), Program::Shell)
        .unwrap_err();
    assert_eq!(kind(err), ErrorKindCode::AlreadyExists);
    assert_eq!(live.tmux.list_panes().unwrap(), before);
}

#[test]
fn a_prefix_of_an_existing_name_is_a_different_session() {
    let Some(live) = Live::start("prefix") else {
        return;
    };
    live.tmux.new_session("work-long", "/tmp").unwrap();
    live.create("work", &live.dir("p"), Program::Shell).unwrap();
    let mut names = live.sessions();
    names.sort();
    assert_eq!(names, ["work", "work-long"]);
}

#[test]
fn a_missing_or_non_directory_is_refused_and_nothing_is_created() {
    let Some(live) = Live::start("dir") else {
        return;
    };
    let file = live.root.join("a-file");
    std::fs::write(&file, "x").unwrap();
    for bad in [
        live.root.join("missing").to_string_lossy().into_owned(),
        file.to_string_lossy().into_owned(),
        "~/also-missing".to_owned(),
    ] {
        for program in [Program::Shell, Program::Claude] {
            let err = live.create("work", &bad, program).unwrap_err();
            assert_eq!(kind(err), ErrorKindCode::InvalidDirectory, "{bad}");
        }
    }
    assert!(live.sessions().is_empty());
    assert!(!live.root.join("missing").exists(), "nothing is created");
}

#[test]
fn tilde_means_the_nodes_home() {
    let Some(live) = Live::start("tilde") else {
        return;
    };
    live.dir("proj");
    live.create("work", "~/proj", Program::Shell).unwrap();
    let panes = live.tmux.list_panes().unwrap();
    assert_eq!(
        canonical(&panes[0].current_path),
        canonical(&live.root.join("proj").to_string_lossy())
    );
}

#[test]
fn claude_missing_from_the_path_is_a_typed_error_and_creates_nothing() {
    let Some(live) = Live::start("nopath") else {
        return;
    };
    let err = live
        .create("agent", &live.dir("p"), Program::Claude)
        .unwrap_err();
    assert_eq!(kind(err), ErrorKindCode::ProgramUnavailable);
    assert!(live.sessions().is_empty());
}

#[test]
fn a_non_executable_claude_is_not_launched() {
    let Some(live) = Live::start("noexec") else {
        return;
    };
    live.script("claude", "sleep 60");
    let path = live.root.join("bin/claude");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let err = live
        .create("agent", &live.dir("p"), Program::Claude)
        .unwrap_err();
    assert_eq!(kind(err), ErrorKindCode::ProgramUnavailable);
    assert!(live.sessions().is_empty());
}

#[test]
fn a_program_that_fails_to_launch_leaves_no_new_session_and_spares_the_rest() {
    let Some(live) = Live::start("crash") else {
        return;
    };
    live.tmux.new_session("keep", "/tmp").unwrap();
    live.script("claude", "exit 1");
    let err = live
        .create("agent", &live.dir("p"), Program::Claude)
        .unwrap_err();
    assert_eq!(kind(err), ErrorKindCode::ProgramUnavailable);
    assert_eq!(live.sessions(), ["keep"]);
}

#[test]
fn a_name_that_is_not_a_plain_name_never_reaches_tmux() {
    let Some(live) = Live::start("name") else {
        return;
    };
    for bad in ["", "a b", "a;b", "a.b", "-x", "$(touch pwned)", "a\nb"] {
        let err = live
            .create(bad, &live.dir("p"), Program::Shell)
            .unwrap_err();
        assert_eq!(kind(err), ErrorKindCode::InvalidRequest, "{bad:?}");
    }
    assert!(live.sessions().is_empty());
}

#[test]
fn an_unknown_server_is_refused() {
    let Some(live) = Live::start("server") else {
        return;
    };
    let err = live
        .servers
        .create_session(&SessionRequest {
            server: ServerId::new("elsewhere"),
            name: "work".into(),
            dir: live.dir("p"),
            program: Program::Shell,
        })
        .unwrap_err();
    assert_eq!(kind(err), ErrorKindCode::InvalidRequest);
}
