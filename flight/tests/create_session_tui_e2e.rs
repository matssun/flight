// SPDX-License-Identifier: MIT

//! The "New session" form, driven by keys in the real dashboard (a pseudo-terminal) against an
//! orchestrator and a node on a private tmux server. Never touches the default tmux server.

mod support;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn n_opens_the_form_and_a_filled_form_creates_a_session_that_appears_selected() {
    let base = PathBuf::from(format!("/tmp/fl-ct-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base dir");
    let _cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(scratch.join("work")).expect("scratch");
    let work = scratch.join("work").canonicalize().expect("canonical");
    let (node_cfg, ui_cfg) = (base.join("n"), base.join("u"));

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
        assert!(out.status.success());
    }
    let socket = format!("fl-ct-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let sock = Sock::Named(socket.clone());
    let _node = Proc(
        flight()
            .args(["node", "run", "--interval", "1", "--socket", &socket])
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
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    let mut child = pair.slave.spawn_command(cmd).expect("dashboard");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let mut writer = pair.master.take_writer().expect("writer");
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
    let seen = |needle: &str| {
        screen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .text()
            .contains(needle)
    };

    wait("the dashboard to show the node and the hint", || {
        seen("mini-e2e") && seen("n New session")
    });

    // Open the form: the fields are labelled and the buttons are there.
    writer.write_all(b"n").expect("n");
    wait("the form", || {
        seen("New session") && seen("Directory:") && seen("[ Create ]")
    });

    // An empty name is explained inside the form and does not close it.
    for key in [&b"\t"[..], b"\t", b"\t", b"\r"] {
        writer.write_all(key).expect("key");
    }
    wait("the name to be asked for", || {
        seen("Enter a name for the session.")
    });

    // Fill it in: name, then the directory, then Shell (right arrow), then Create.
    for _ in 0..3 {
        writer.write_all(b"\x7f").expect("bs");
    }
    writer.write_all(b"quick").expect("name");
    writer.write_all(b"\t").expect("tab");
    writer.write_all(&[0x7f; 4]).expect("clear dir");
    writer
        .write_all(work.to_string_lossy().as_bytes())
        .expect("dir");
    writer.write_all(b"\t\x1b[C").expect("shell");
    writer.write_all(b"\t\r").expect("create");

    wait("tmux to have the session", || {
        tmux(&sock, &["list-sessions", "-F", "#{session_name}"])
            .unwrap_or_default()
            .lines()
            .any(|l| l == "quick")
    });
    let cwd = tmux(
        &sock,
        &[
            "display-message",
            "-p",
            "-t",
            "=quick:",
            "#{pane_current_path}",
        ],
    )
    .unwrap_or_default();
    assert_eq!(
        PathBuf::from(cwd.trim()).canonicalize().unwrap_or_default(),
        work
    );
    wait(
        "the dashboard to confirm, close the form and select the new session",
        || seen("Created session quick on mini-e2e.") && !seen("[ Create ]") && seen("> quick"),
    );

    // Back on the dashboard: the form is gone and `q` quits again.
    writer.write_all(b"q").expect("quit");
    wait("the dashboard to exit", || {
        child.try_wait().ok().flatten().is_some()
    });
}
