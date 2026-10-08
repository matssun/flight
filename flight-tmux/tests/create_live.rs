// SPDX-License-Identifier: MIT

//! Session creation against a real tmux server on a private socket. Skipped when tmux is
//! absent. Every call carries `-S <unique private path>`: the default server is never used.

use flight_tmux::{CreateError, Launch, NewSession, Tmux, TmuxEndpoint};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Server {
    tmux: Tmux,
    socket: PathBuf,
}

impl Server {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let socket =
            std::env::temp_dir().join(format!("flight-create-{tag}-{}.sock", std::process::id()));
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

fn script(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().into_owned()
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flight-create-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn a_shell_session_starts_in_the_directory_and_is_marked() {
    let Some(s) = Server::start("shell") else {
        return;
    };
    let dir = scratch("shell");
    let id = s
        .tmux
        .create_session(&NewSession {
            name: "work".into(),
            dir: dir.to_string_lossy().into_owned(),
            launch: Launch::DefaultShell,
        })
        .unwrap();
    assert!(id.starts_with('$'));
    let panes = s.tmux.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].session_name, "work");
    assert!(panes[0].flight_session);
    assert_eq!(
        Path::new(&panes[0].current_path).canonicalize().unwrap(),
        dir
    );
}

#[test]
fn a_program_runs_directly_in_the_directory() {
    let Some(s) = Server::start("prog") else {
        return;
    };
    let dir = scratch("prog");
    // Records where it ran, then stays alive like an interactive agent.
    let program = script(
        &dir,
        "claude",
        &format!("pwd > {0}/ran; sleep 30", dir.display()),
    );
    s.tmux
        .create_session(&NewSession {
            name: "agent".into(),
            dir: dir.to_string_lossy().into_owned(),
            launch: Launch::Program {
                argv: vec![program],
            },
        })
        .unwrap();
    let wait = || {
        for _ in 0..50 {
            if dir.join("ran").exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    wait();
    let ran = std::fs::read_to_string(dir.join("ran")).unwrap();
    assert_eq!(Path::new(ran.trim()).canonicalize().unwrap(), dir);
}

#[test]
fn an_existing_session_is_refused_and_left_alone() {
    let Some(s) = Server::start("dup") else {
        return;
    };
    s.tmux.new_session("keep", "/tmp").unwrap();
    let before = s.tmux.list_panes().unwrap();
    let err = s
        .tmux
        .create_session(&NewSession {
            name: "keep".into(),
            dir: "/tmp".into(),
            launch: Launch::DefaultShell,
        })
        .unwrap_err();
    assert!(matches!(err, CreateError::AlreadyExists), "{err}");
    assert_eq!(s.tmux.list_panes().unwrap(), before);
    assert!(!before[0].flight_session);
}

#[test]
fn a_program_that_fails_at_once_leaves_no_session_and_spares_others() {
    let Some(s) = Server::start("fail") else {
        return;
    };
    let dir = scratch("fail");
    s.tmux.new_session("other", "/tmp").unwrap();
    let program = script(&dir, "claude", "exit 3");
    let err = s
        .tmux
        .create_session(&NewSession {
            name: "doomed".into(),
            dir: dir.to_string_lossy().into_owned(),
            launch: Launch::Program {
                argv: vec![program],
            },
        })
        .unwrap_err();
    assert!(matches!(err, CreateError::Exited), "{err}");
    assert!(!s.tmux.has_session("doomed"));
    assert!(s.tmux.has_session("other"));
}

#[test]
fn the_only_session_failing_leaves_nothing_and_no_error_about_the_server() {
    let Some(s) = Server::start("solo") else {
        return;
    };
    let dir = scratch("solo");
    let program = script(&dir, "claude", "exit 1");
    let err = s
        .tmux
        .create_session(&NewSession {
            name: "doomed".into(),
            dir: dir.to_string_lossy().into_owned(),
            launch: Launch::Program {
                argv: vec![program],
            },
        })
        .unwrap_err();
    assert!(matches!(err, CreateError::Exited), "{err}");
    assert!(!s.tmux.has_session("doomed"));
}
