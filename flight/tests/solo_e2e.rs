// SPDX-License-Identifier: MIT

//! `flight` with no setup: the first run starts everything, shows the dashboard with the
//! empty-state help, and quitting stops what it started. A private session server and config
//! directory; nothing of the machine's own is touched.

mod support;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

struct Solo {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<Screen>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Drop for Solo {
    fn drop(&mut self) {
        // A failed test must not leave its orchestrator holding the port for the next one.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Solo {
    fn text(&self) -> String {
        self.screen.lock().unwrap_or_else(|p| p.into_inner()).text()
    }

    fn wait_for(&self, what: &str, check: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let text = self.text();
            if check(&text) {
                return;
            }
            assert!(Instant::now() < deadline, "no {what}:\n{text}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("send");
        self.writer.flush().expect("flush");
        std::thread::sleep(Duration::from_millis(150));
    }

    fn quit(&mut self) {
        self.send(b"q");
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().ok().flatten().is_none() {
            assert!(Instant::now() < deadline, "flight did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// `flight solo` on a private config directory and session server, on a pseudo-terminal.
fn start_solo(base: &std::path::Path, socket: &str, path: &str) -> Solo {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty");
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_flight"));
    cmd.args(["solo", "--socket", socket, "--config-dir"]);
    cmd.arg(base);
    cmd.env_clear();
    cmd.env("TERM", "xterm-256color");
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("PATH", path);
    let child = pair.slave.spawn_command(cmd).expect("flight");
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
    Solo {
        child,
        writer,
        screen,
        _master: pair.master,
    }
}

/// Plain flight starts its orchestrator on a fixed port, so these tests take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn have_tmux() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok()
}

#[test]
fn plain_flight_sets_itself_up_shows_the_dashboard_and_stops_what_it_started() {
    if !have_tmux() {
        return;
    }
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let base = PathBuf::from(format!("/tmp/fl-solo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _cleanup = TempDir(base.clone());
    let socket = format!("fl-solo-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let _sock = Sock::Named(socket.clone());

    let mut solo = start_solo(&base, &socket, &std::env::var("PATH").unwrap_or_default());
    solo.wait_for("dashboard", |t| {
        t.contains("No Flight workspaces yet") && t.contains("Connected hosts")
    });
    assert!(base.join("orchestrator").join("admin.sock").exists());

    solo.quit();
    // What it started is stopped: nothing answers on the orchestrator's admin socket.
    std::thread::sleep(Duration::from_millis(500));
    let still_up = std::os::unix::net::UnixStream::connect(base.join("orchestrator/admin.sock"));
    assert!(still_up.is_err(), "the orchestrator was left running");
}

#[test]
fn a_workspace_made_in_plain_flight_is_saved_like_any_other() {
    if !have_tmux() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let base = PathBuf::from(format!("/tmp/fl-solo-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _cleanup = TempDir(base.clone());
    let socket = format!("fl-solo-save-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let _sock = Sock::Named(socket.clone());
    // A `claude` that only waits, so the form has an agent to offer.
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::write(bin.join("claude"), "#!/bin/sh\nexec sleep 3600\n").expect("claude");
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755))
        .expect("chmod");
    let work = base.join("work");
    std::fs::create_dir_all(&work).expect("work");
    let work = work.canonicalize().expect("canonical");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut solo = start_solo(&base, &socket, &path);
    // The node has to be connected before the form has a host to create on.
    solo.wait_for("dashboard", |t| {
        t.contains("Connected hosts") && t.contains("n New")
    });
    std::thread::sleep(Duration::from_secs(1));
    solo.send(b"n");
    solo.wait_for("form", |t| t.contains("New workspace"));
    for _ in 0..3 {
        solo.send(b"\x7f");
    }
    solo.send(b"quick");
    solo.send(b"\t");
    solo.send(&[0x7f; 4]);
    solo.send(work.to_string_lossy().as_bytes());
    solo.send(b"\t");
    solo.send(b"\t");
    solo.send(b"\r");
    solo.wait_for("workspace", |t| t.contains("Created workspace quick"));
    // The node saves what it sees; give its reporter a moment.
    let saved = base.join("node").join("workspaces.toml");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(&saved).is_ok_and(|t| t.contains("quick")) {
        assert!(
            Instant::now() < deadline,
            "nothing saved in {}:\n{}",
            saved.display(),
            solo.text()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    solo.quit();
}
