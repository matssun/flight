// SPDX-License-Identifier: MIT

//! A workspace and its companion shell, driven by keys in the real dashboard (a pseudo-terminal)
//! against an orchestrator and a node on a private tmux server: create the workspace, be offered
//! a shell, create it, switch between agent and shell inside the session, come back. Never
//! touches the default tmux server.

mod support;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

/// A `claude` that only waits, first on the node's `PATH`: the form offers an agent, and the
/// test needs one that can run anywhere.
fn path_with_fake_claude(base: &std::path::Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let claude = bin.join("claude");
    std::fs::write(&claude, "#!/bin/sh\nexec sleep 3600\n").expect("fake claude");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Rig {
    sock: Sock,
    work: PathBuf,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<Screen>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _keep: Vec<Box<dyn std::any::Any>>,
}

impl Rig {
    fn seen(&self, needle: &str) -> bool {
        self.screen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .text()
            .contains(needle)
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("send");
        self.writer.flush().expect("flush");
        // Keys arrive one thing at a time, as typed.
        std::thread::sleep(Duration::from_millis(150));
    }

    fn tmux(&self, args: &[&str]) -> String {
        tmux(&self.sock, args).unwrap_or_default()
    }

    /// Terminals attached to the session: the node's own control connection is not one.
    fn clients(&self) -> usize {
        self.tmux(&["list-clients", "-F", "#{client_control_mode}"])
            .lines()
            .filter(|l| l.trim() == "0")
            .count()
    }
}

fn boot(tag: &str) -> Rig {
    let base = PathBuf::from(format!("/tmp/fl-ws-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base dir");
    let cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(scratch.join("work")).expect("scratch");
    let work = scratch.join("work").canonicalize().expect("canonical");
    let (node_cfg, ui_cfg) = (base.join("n"), base.join("u"));

    let (orch, _addr) = start_orchestrator(&base);
    for (role, name, cfg) in [("node", "mini-e2e", &node_cfg), ("ui", "laptop", &ui_cfg)] {
        let bundle = run_ok(&base, &["orchestrator", "enrollment", "create"])
            .trim()
            .to_owned();
        let out = flight()
            .args([role, "join", "--name", name, "--config-dir"])
            .arg(cfg)
            .args(bundle.split_whitespace())
            .output()
            .expect("join");
        assert!(out.status.success());
    }
    let socket = format!("fl-ws-{tag}-{}", std::process::id());
    let file = NamedSocketFile(socket.clone());
    let sock = Sock::Named(socket.clone());
    let node = Proc(
        flight()
            .args(["node", "run", "--interval", "1", "--socket", &socket])
            .env("PATH", path_with_fake_claude(&base))
            .arg("--config-dir")
            .arg(&node_cfg)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start node"),
    );
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty");
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_flight"));
    cmd.args(["ui", "run", "--refresh", "1", "--config-dir"]);
    cmd.arg(&ui_cfg);
    cmd.env_clear();
    cmd.env("TERM", "xterm-256color");
    if let Ok(t) = std::env::var("FLIGHT_TRACE_FILE") {
        cmd.env("FLIGHT_TRACE_FILE", t);
    }
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    let child = pair.slave.spawn_command(cmd).expect("dashboard");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let writer = pair.master.take_writer().expect("writer");
    let screen = Arc::new(Mutex::new(Screen::new(30, 110)));
    {
        let screen = screen.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let chunk = String::from_utf8_lossy(buf.get(..n).unwrap_or_default()).into_owned();
                screen
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .feed(&chunk);
            }
        });
    }
    Rig {
        sock,
        work,
        writer,
        screen,
        child,
        _keep: vec![
            Box::new(pair.master),
            Box::new(node),
            Box::new(file),
            Box::new(orch),
            Box::new(cleanup),
        ],
    }
}

#[test]
fn a_workspace_gets_a_shell_in_its_directory_and_agent_and_shell_switch_without_the_dashboard() {
    let mut rig = boot("basic");
    wait("the dashboard", || {
        rig.seen("mini-e2e") && rig.seen("n New")
    });

    // Create the workspace through the form: name, directory, the default agent.
    rig.send(b"n");
    wait("the form", || rig.seen("New workspace"));
    for _ in 0..3 {
        rig.send(b"\x7f");
    }
    rig.send(b"quick");
    rig.send(b"\t");
    rig.send(&[0x7f; 4]);
    let dir = rig.work.to_string_lossy().into_owned();
    rig.send(dir.as_bytes());
    rig.send(b"\t");
    rig.send(b"\t");
    rig.send(b"\r");
    wait("the workspace to be listed and selected", || {
        rig.seen("Created workspace quick on mini-e2e.") && rig.seen("none yet")
    });
    assert_eq!(
        rig.tmux(&["list-windows", "-t", "=quick:"]).lines().count(),
        1
    );

    // `s`: no shell yet. The offer names the workspace, and nothing is created until yes.
    rig.send(b"s");
    wait("the offer", || rig.seen("quick has no shell yet"));
    assert_eq!(
        rig.tmux(&["list-windows", "-t", "=quick:"]).lines().count(),
        1,
        "asking is not creating"
    );
    rig.send(b"\r");

    // The shell is made in the workspace's directory and opened at once.
    wait("a second window", || {
        rig.tmux(&["list-windows", "-t", "=quick:"]).lines().count() == 2
    });
    let paths = rig.tmux(&[
        "list-panes",
        "-s",
        "-t",
        "=quick:",
        "-F",
        "#{window_name} #{pane_current_path}",
    ]);
    let shell_line = paths
        .lines()
        .find(|l| l.starts_with("shell "))
        .unwrap_or_else(|| panic!("no shell window in {paths}"));
    let shell_dir = PathBuf::from(shell_line.trim_start_matches("shell "));
    assert_eq!(shell_dir.canonicalize().unwrap_or_default(), rig.work);
    // On screen means the session's own status line reached the user's terminal.
    wait("the shell on screen", || {
        rig.clients() == 1 && rig.seen("1:shell*")
    });

    // Work in the shell.
    rig.send(b"echo from-the-shell-$((6*7))\r");
    wait("the command to have run in the shell", || {
        rig.tmux(&["capture-pane", "-p", "-t", "=quick:shell"])
            .contains("from-the-shell-42")
    });

    // Ctrl-Space a: the agent, without passing through the dashboard's list.
    let started = std::time::Instant::now();
    rig.send(b"\x00a");
    wait("the agent shown", || {
        rig.clients() == 1 && rig.seen("0:agent*")
    });
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "switching took {:?}",
        started.elapsed()
    );
    // Ctrl-Space s: the shell again, with what was in it.
    rig.send(b"\x00s");
    wait("the shell shown again", || {
        rig.clients() == 1 && rig.seen("1:shell*")
    });
    assert!(rig
        .tmux(&["capture-pane", "-p", "-t", "=quick:shell"])
        .contains("from-the-shell-42"));

    // Ctrl-Space q: back at the dashboard, which shows the workspace with both surfaces.
    rig.send(b"\x00q");
    wait("the dashboard again", || {
        rig.seen("Workspaces") && rig.seen("s Shell") && rig.clients() == 0
    });

    // Leaving the interface leaves the work running.
    rig.send(b"q");
    wait("the dashboard to exit", || {
        rig.child.try_wait().ok().flatten().is_some()
    });
    assert_eq!(
        rig.tmux(&["list-windows", "-t", "=quick:"]).lines().count(),
        2
    );
}

/// Create workspace `quick` through the form and give it a shell, leaving the shell on screen.
fn workspace_with_shell(rig: &mut Rig) {
    wait("the dashboard", || {
        rig.seen("mini-e2e") && rig.seen("n New")
    });
    rig.send(b"n");
    wait("the form", || rig.seen("New workspace"));
    for _ in 0..3 {
        rig.send(b"\x7f");
    }
    rig.send(b"quick");
    rig.send(b"\t");
    rig.send(&[0x7f; 4]);
    let dir = rig.work.to_string_lossy().into_owned();
    rig.send(dir.as_bytes());
    rig.send(b"\t");
    rig.send(b"\t");
    rig.send(b"\r");
    wait("the workspace to be listed and selected", || {
        rig.seen("Created workspace quick on mini-e2e.") && rig.seen("none yet")
    });
    rig.send(b"s");
    wait("the offer", || rig.seen("quick has no shell yet"));
    rig.send(b"\r");
    wait("the shell on screen", || {
        rig.clients() == 1 && rig.seen("1:shell*")
    });
}

fn pane(rig: &Rig, window: &str) -> String {
    rig.tmux(&["capture-pane", "-p", "-t", &format!("=quick:{window}")])
}

#[test]
fn keys_typed_right_after_opening_or_switching_reach_the_surface_they_were_typed_for() {
    let mut rig = boot("typed");
    workspace_with_shell(&mut rig);
    rig.send(b"\x00q");
    wait("the dashboard again", || {
        rig.seen("Workspaces") && rig.seen("s Shell") && rig.clients() == 0
    });

    // `s` opens the shell, and the rest is typed at once, before anything is on screen: the
    // dashboard must not read any of it as commands (it has keys for most of these letters).
    rig.writer
        .write_all(b"secho typed-ahead-$((6*7))\r")
        .expect("keys");
    rig.writer.flush().expect("flush");
    wait("the shell has what was typed", || {
        pane(&rig, "shell").contains("typed-ahead-42")
    });
    assert!(!pane(&rig, "agent").contains("typed-ahead"), "misdelivered");
    wait("the shell on screen", || {
        rig.clients() == 1 && rig.seen("1:shell*")
    });

    // Switch away and back in one burst, with input for each surface between the switches.
    rig.writer
        .write_all(b"echo one-$((1+1))\r\x00aFOR-THE-AGENT\x00secho two-$((2+2))\r")
        .expect("keys");
    rig.writer.flush().expect("flush");
    wait("both parts reached the shell", || {
        let shell = pane(&rig, "shell");
        shell.contains("one-2") && shell.contains("two-4")
    });
    wait("the agent got its part", || {
        pane(&rig, "agent").contains("FOR-THE-AGENT")
    });
    let shell = pane(&rig, "shell");
    assert!(!shell.contains("FOR-THE-AGENT"), "misdelivered: {shell}");
    assert!(
        shell
            .find("one-2")
            .zip(shell.find("two-4"))
            .is_some_and(|(a, b)| a < b),
        "out of order: {shell}"
    );
    // The surfaces are both still there and exactly one client shows one of them (the one that
    // showed the other surface is let go a moment after the switch).
    wait("only one client is left", || rig.clients() == 1);
    assert_eq!(
        rig.tmux(&["list-windows", "-t", "=quick:"]).lines().count(),
        2
    );
}
