// SPDX-License-Identifier: MIT

//! Enter on a pane of another machine, with the real binaries: an orchestrator, a node on a
//! private tmux server, and the dashboard in a pseudo-terminal whose configuration has no node
//! (so every pane is remote). A fake `ssh` that records any call sits first on the dashboard's
//! PATH: the whole session must work without it ever being run. Never touches the default
//! tmux server.

mod support;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn enter_on_a_remote_pane_shows_it_over_flight_without_ssh_and_returns_to_the_dashboard() {
    let base = PathBuf::from(format!("/tmp/fl-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base dir");
    let _cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let Some(claude) = fake_claude(&scratch) else {
        return;
    };
    std::fs::write(scratch.join("screen.txt"), PERMIT_SCREEN).expect("screen");
    let (node_cfg, ui_cfg) = (base.join("n"), base.join("u"));

    // A fake ssh that records being run. It must never be.
    let bin = scratch.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let ssh_log = scratch.join("ssh-was-run");
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        format!("#!/bin/sh\necho \"$@\" >> {}\nexit 99\n", ssh_log.display()),
    )
    .expect("fake ssh");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let (_orch, _addr) = start_orchestrator(&base);
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
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let socket = format!("fl-rt-agents-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let agents = Sock::Named(socket.clone());
    let run = format!(
        "cat {}; exec {}",
        scratch.join("screen.txt").display(),
        claude.display()
    );
    tmux(
        &agents,
        &[
            "new-session",
            "-d",
            "-s",
            "nga",
            "-x",
            "100",
            "-y",
            "30",
            "-c",
            "/tmp",
            &run,
        ],
    )
    .expect("agent session");
    wait_for(&agents, "nga", &["Do you want to proceed?"]);
    let _node = Proc(
        flight()
            .args([
                "node",
                "run",
                "--interval",
                "1",
                "--socket",
                &socket,
                "--config-dir",
            ])
            .arg(&node_cfg)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start node"),
    );
    wait_for_dashboard(&ui_cfg);

    // The dashboard, in a pseudo-terminal.
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
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    let mut child = pair.slave.spawn_command(cmd).expect("dashboard");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let mut writer = pair.master.take_writer().expect("writer");
    let screen = Arc::new(Mutex::new(String::new()));
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
                    .push_str(&chunk);
            }
        });
    }
    let seen = |needle: &str| {
        screen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(needle)
    };
    let clients = || {
        // The node's own observer is a control client; only terminals count.
        tmux(
            &agents,
            &[
                "list-clients",
                "-F",
                "#{client_session} #{client_control_mode}",
            ],
        )
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_suffix(" 0").map(str::to_owned))
        .collect::<Vec<_>>()
    };

    wait("the dashboard lists the agent", || seen("nga"));
    assert!(clients().is_empty(), "the dashboard is not a tmux client");

    // Enter: reveal, open a terminal, show it.
    writer.write_all(b"\r").expect("enter");
    wait("a tmux client on the node, carried over Flight", || {
        clients() == vec!["nga".to_owned()]
    });
    // Leave with the local escape; the dashboard comes back and says why.
    std::thread::sleep(Duration::from_millis(500));
    let at_escape = screen.lock().unwrap_or_else(|p| p.into_inner()).len();
    writer.write_all(b"\x00q").expect("escape");
    wait("the node's tmux client to go", || clients().is_empty());
    wait("the dashboard to come back with the reason", || {
        let all = screen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        all.get(at_escape..)
            .is_some_and(|after| after.contains("terminal: left the terminal"))
    });

    writer.write_all(b"q").expect("quit");
    wait("the dashboard to exit", || {
        child.try_wait().ok().flatten().is_some()
    });
    assert!(
        !ssh_log.exists(),
        "ssh was run: {}",
        std::fs::read_to_string(&ssh_log).unwrap_or_default()
    );
}

fn wait_for_dashboard(ui_cfg: &std::path::Path) {
    wait("the orchestrator to show the node's agent", || {
        let out = flight()
            .args(["ui", "--once", "--config-dir"])
            .arg(ui_cfg)
            .output()
            .expect("ui");
        String::from_utf8_lossy(&out.stdout).contains("nga")
    });
}
