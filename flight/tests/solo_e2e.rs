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

#[test]
fn plain_flight_sets_itself_up_shows_the_dashboard_and_stops_what_it_started() {
    if std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .is_err()
    {
        return;
    }
    let base = PathBuf::from(format!("/tmp/fl-solo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _cleanup = TempDir(base.clone());
    let socket = format!("fl-solo-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let _sock = Sock::Named(socket.clone());

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty");
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_flight"));
    cmd.args(["solo", "--socket", &socket, "--config-dir"]);
    cmd.arg(&base);
    cmd.env_clear();
    cmd.env("TERM", "xterm-256color");
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    let mut child = pair.slave.spawn_command(cmd).expect("flight");
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
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = screen.lock().unwrap_or_else(|p| p.into_inner()).text();
        if text.contains("No Flight workspaces yet") && text.contains("Connected hosts") {
            break;
        }
        assert!(Instant::now() < deadline, "no dashboard:\n{text}");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(base.join("orchestrator").join("admin.sock").exists());

    writer.write_all(b"q").expect("quit");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().ok().flatten().is_none() {
        assert!(Instant::now() < deadline, "flight did not exit");
        std::thread::sleep(Duration::from_millis(50));
    }
    // What it started is stopped: nothing answers on the orchestrator's admin socket.
    std::thread::sleep(Duration::from_millis(500));
    let still_up = std::os::unix::net::UnixStream::connect(base.join("orchestrator/admin.sock"));
    assert!(still_up.is_err(), "the orchestrator was left running");
}
