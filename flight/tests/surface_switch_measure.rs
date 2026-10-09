// SPDX-License-Identifier: MIT

//! Measures the surface switch (ADR-009) with the real binaries: the time from `Ctrl-Space a|s`
//! until the other surface is on the user's screen, and the resources the path holds. Ignored by
//! default; run `cargo test --release -p flight --test surface_switch_measure -- --ignored --nocapture`.
//! Never touches the default tmux server.

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

fn boot() -> Rig {
    let base = PathBuf::from(format!("/tmp/fl-ws-{}", std::process::id()));
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
        let bundle = through_proxy(&bundle, &_addr);
        let out = flight()
            .args([role, "join", "--name", name, "--config-dir"])
            .arg(cfg)
            .args(bundle.split_whitespace())
            .output()
            .expect("join");
        assert!(out.status.success());
    }
    let socket = format!("fl-ws-{}", std::process::id());
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
    let _child = pair.slave.spawn_command(cmd).expect("dashboard");
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
        _keep: vec![
            Box::new(pair.master),
            Box::new(node),
            Box::new(file),
            Box::new(orch),
            Box::new(cleanup),
        ],
    }
}

/// Resident set (KiB), thread count and open descriptors of a pid, by `ps` and `lsof`.
fn resources(pid: u32) -> (u64, usize, usize) {
    let ps = |field: &str| {
        let out = std::process::Command::new("ps")
            .args(["-o", field, "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        out.lines()
            .nth(1)
            .unwrap_or("0")
            .trim()
            .parse::<u64>()
            .unwrap_or(0)
    };
    let fds = std::process::Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0);
    let threads = std::process::Command::new("ps")
        .args(["-M", "-p", &pid.to_string()])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .count()
                .saturating_sub(1)
        })
        .unwrap_or(0);
    (ps("rss"), threads, fds)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted.get(idx).copied().unwrap_or(0.0)
}

#[test]
#[ignore = "measurement, not a pass/fail test"]
fn measure_the_surface_switch() {
    let mut rig = boot();
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
    wait("the workspace", || {
        rig.seen("Created workspace quick on mini-e2e.") && rig.seen("none yet")
    });
    rig.send(b"s");
    wait("the offer", || rig.seen("quick has no shell yet"));
    rig.send(b"\r");
    wait("the shell on screen", || {
        rig.clients() == 1 && rig.seen("1:shell*")
    });

    report_resources("before");
    // Alternate agent and shell; time each from the key to the other surface's status marker.
    let rounds: usize = std::env::var("FLIGHT_MEASURE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let mut samples = Vec::new();
    for i in 0..rounds {
        let (key, marker): (&[u8], &str) = if i % 2 == 0 {
            (b"\x00a", "0:agent*")
        } else {
            (b"\x00s", "1:shell*")
        };
        let start = Instant::now();
        rig.writer.write_all(key).expect("key");
        rig.writer.flush().expect("flush");
        let deadline = start + Duration::from_secs(20);
        while !rig.seen(marker) {
            assert!(Instant::now() < deadline, "switch {i} did not complete");
            std::thread::sleep(Duration::from_micros(500));
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        // Settle so the next switch starts from a quiet system.
        std::thread::sleep(Duration::from_millis(400));
    }
    let mut sorted = samples.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    println!(
        "SWITCH samples ms: {:?}",
        samples.iter().map(|s| s.round() as u64).collect::<Vec<_>>()
    );
    println!(
        "SWITCH n={} min={:.0} p50={:.0} p95={:.0} max={:.0}",
        sorted.len(),
        sorted.first().copied().unwrap_or(0.0),
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.95),
        sorted.last().copied().unwrap_or(0.0)
    );
    println!("tmux clients attached: {}", rig.clients());
    report_resources("after");
}

/// Memory, threads and descriptors of the Flight processes (orchestrator, node, dashboard).
fn report_resources(when: &str) {
    // Only the processes of this run: they were all started with this run's directory.
    let pids = std::process::Command::new("pgrep")
        .args(["-f", &format!("fl-ws-{}", std::process::id())])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    for pid in pids.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
        let (rss, threads, fds) = resources(pid);
        println!("RES {when} pid {pid}: rss={rss} KiB threads={threads} fds={fds}");
    }
}
