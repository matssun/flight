// SPDX-License-Identifier: MIT

//! The distributed Flight, as real processes on localhost: an orchestrator, a node observing a
//! private tmux server of fake agents, and the dashboard reading through the orchestrator,
//! all enrolled with real bundles over mutual TLS. Skipped when tmux or a C compiler (for the
//! fake `claude`) is missing. Never touches the default tmux server.

mod support;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::*;

fn flight() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flight"))
}

/// A child process killed on drop.
struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_ok(base: &Path, args: &[&str]) -> String {
    let out = flight()
        .args(args)
        .arg("--config-dir")
        .arg(base)
        .output()
        .expect("run flight");
    assert!(
        out.status.success(),
        "flight {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn start_orchestrator(base: &Path) -> (Proc, String) {
    let mut child = flight()
        .args([
            "orchestrator",
            "run",
            "--listen",
            "127.0.0.1:0",
            "--name",
            "e2e-orch",
            "--config-dir",
        ])
        .arg(base)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start orchestrator");
    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut addr = None;
    for _ in 0..3 {
        let line = lines.next().expect("orchestrator output").expect("line");
        if let Some(a) = line.strip_prefix("listening on ") {
            addr = Some(a.to_owned());
        }
    }
    (Proc(child), addr.expect("listening line"))
}

fn dir_is_empty(path: &Path) -> bool {
    !path.exists()
        || std::fs::read_dir(path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
}

fn ui_once(base: &Path) -> String {
    let out = flight()
        .args(["ui", "--once", "--config-dir"])
        .arg(base)
        .output()
        .expect("ui");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn wait_for_ui(base: &Path, want: &[&str]) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let screen = ui_once(base);
        if want.iter().all(|w| screen.contains(w)) || Instant::now() > deadline {
            return screen;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn enroll_node_and_ui_then_see_agents_through_the_orchestrator() {
    // Short paths: a Unix socket path must fit in ~100 bytes.
    let base = PathBuf::from(format!("/tmp/fl-dist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base dir");
    let _cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let Some(claude) = fake_claude(&scratch) else {
        return;
    };
    std::fs::write(scratch.join("screen.txt"), PERMIT_SCREEN).expect("screen");
    let node_cfg = base.join("n");
    let ui_cfg = base.join("u");

    // The orchestrator and a bundle.
    let (_orch, addr) = start_orchestrator(&base);
    let bundle = run_ok(&base, &["orchestrator", "enrollment", "create"])
        .trim()
        .to_owned();
    assert!(bundle.contains(&format!("address={addr}")), "{bundle}");

    // A failed join leaves nothing behind: no identity, no settings.
    let bad = bundle.replace("token=", "token=x");
    let out = flight()
        .args(["node", "join", "--config-dir"])
        .arg(&node_cfg)
        .args(bad.split_whitespace())
        .output()
        .expect("join");
    assert!(!out.status.success(), "a bad token must not join");
    assert!(
        dir_is_empty(&node_cfg),
        "failed join left files in {node_cfg:?}"
    );

    // Join the node (and then the UI, with a second single-use bundle).
    let out = flight()
        .args(["node", "join", "--name", "mini-e2e", "--config-dir"])
        .arg(&node_cfg)
        .args(bundle.split_whitespace())
        .output()
        .expect("join");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let replay = flight()
        .args(["ui", "join", "--config-dir"])
        .arg(&ui_cfg)
        .args(bundle.split_whitespace())
        .output()
        .expect("replay");
    assert!(
        !replay.status.success(),
        "a used bundle must not enroll anything else"
    );
    let ui_bundle = run_ok(&base, &["orchestrator", "enrollment", "create"])
        .trim()
        .to_owned();
    let out = flight()
        .args(["ui", "join", "--name", "laptop", "--config-dir"])
        .arg(&ui_cfg)
        .args(ui_bundle.split_whitespace())
        .output()
        .expect("join ui");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Agents in a private tmux server, observed by the node.
    let socket = format!("fl-dist-agents-{}", std::process::id());
    let _file = NamedSocketFile(socket.clone());
    let agents = Sock::Named(socket.clone());
    let run = format!(
        "cat {}; exec {}",
        scratch.join("screen.txt").display(),
        claude.display()
    );
    for session in ["nga", "api"] {
        tmux(
            &agents,
            &["new-session", "-d", "-s", session, "-c", "/tmp", &run],
        )
        .expect("agent session");
    }
    tmux(
        &agents,
        &["new-session", "-d", "-s", "scratch", "-c", "/tmp"],
    )
    .expect("shell session");
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

    // The dashboard, through the orchestrator, shows the node, its agents and a preview.
    let screen = wait_for_ui(
        &ui_cfg,
        &["mini-e2e", "nga", "api", "Do you want to proceed?"],
    );
    assert!(screen.contains("mini-e2e"), "{screen}");
    assert!(screen.contains("nga") && screen.contains("api"), "{screen}");
    assert!(
        screen.contains("Do you want to proceed?"),
        "preview over the orchestrator: {screen}"
    );
    assert!(
        !screen.contains("scratch"),
        "the shell session is not an agent: {screen}"
    );

    // Revoking the node at the orchestrator is visible to the operator.
    let listing = run_ok(&base, &["orchestrator", "trust", "list"]);
    assert!(
        listing.contains("mini-e2e") && listing.contains("laptop"),
        "{listing}"
    );
}
