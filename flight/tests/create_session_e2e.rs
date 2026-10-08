// SPDX-License-Identifier: MIT

//! Creating sessions across the real distributed path, with the real binaries: an orchestrator,
//! two nodes on private tmux servers (each with a fake `claude` first on its PATH), and the
//! dashboard's own backend as the UI. A fake `ssh` that records any call sits on the nodes'
//! PATH: nothing may run it. Never touches the default tmux server.

mod support;

use flight_client::{ClientConfig, OrchestratedBackend};
use flight_ui::{Backend, CreateFailure, HostHealth, NewSessionRequest, Program, UiSnapshot};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use support::*;

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn join(base: &Path, role: &str, name: &str, cfg: &Path) {
    let bundle = run_ok(base, &["orchestrator", "enrollment", "create"])
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

struct Node {
    label: &'static str,
    sock: Sock,
    _file: NamedSocketFile,
    process: Option<Proc>,
}

fn start_node(base: &Path, label: &'static str, path: &str) -> Node {
    let cfg = base.join(label);
    join(base, "node", label, &cfg);
    let socket = format!("fl-cs-{label}-{}", std::process::id());
    let node = Node {
        label,
        sock: Sock::Named(socket.clone()),
        _file: NamedSocketFile(socket.clone()),
        process: None,
    };
    let process = Proc(
        flight()
            .args(["node", "run", "--interval", "1", "--socket", &socket])
            .arg("--config-dir")
            .arg(&cfg)
            .env("PATH", path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start node"),
    );
    Node {
        process: Some(process),
        ..node
    }
}

fn sessions(node: &Node) -> Vec<String> {
    let mut names: Vec<String> = tmux(&node.sock, &["list-sessions", "-F", "#{session_name}"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

fn host_of(snapshot: &UiSnapshot, label: &str) -> Option<flight_state::HostId> {
    snapshot
        .hosts
        .iter()
        .find(|h| h.label == label && matches!(h.health, HostHealth::Online | HostHealth::NoServer))
        .map(|h| h.host.clone())
}

fn request(
    backend: &mut OrchestratedBackend,
    label: &str,
    name: &str,
    dir: &Path,
    program: Program,
) -> Result<(), CreateFailure> {
    let host = host_of(&backend.snapshot(0), label).expect("node is connected");
    backend.create_session(&NewSessionRequest {
        host,
        host_label: label.to_owned(),
        name: name.to_owned(),
        dir: dir.to_string_lossy().into_owned(),
        program,
    })
}

#[test]
fn a_ui_creates_sessions_on_the_node_it_names_through_the_orchestrator_without_ssh() {
    let base = PathBuf::from(format!("/tmp/fl-cs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base dir");
    let _cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let Some(claude) = fake_claude(&scratch) else {
        return;
    };
    let bin = scratch.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::rename(&claude, bin.join("claude")).expect("claude on the node's path");
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
    let dir_a = scratch.join("work-a");
    let dir_b = scratch.join("work-b");
    for d in [&dir_a, &dir_b] {
        std::fs::create_dir_all(d).expect("work dir");
    }
    let dir_a = dir_a.canonicalize().expect("canonical");
    let dir_b = dir_b.canonicalize().expect("canonical");

    let (_orch, _addr) = start_orchestrator(&base);
    join(&base, "ui", "laptop", &base.join("u"));
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let a = start_node(&base, "node-a", &path);
    let mut b = start_node(&base, "node-b", &path);

    let config = ClientConfig::load(&base.join("u").join("ui")).expect("ui config");
    let mut backend = OrchestratedBackend::start(config).expect("backend");
    wait("both nodes to be connected", || {
        let s = backend.snapshot(0);
        host_of(&s, "node-a").is_some() && host_of(&s, "node-b").is_some()
    });

    // Shell on A, Claude on B: each lands on the node it names and nowhere else.
    request(&mut backend, "node-a", "alpha", &dir_a, Program::Shell).expect("create on a");
    request(&mut backend, "node-b", "beta", &dir_b, Program::Claude).expect("create on b");
    assert_eq!(sessions(&a), ["alpha"], "{} got only its own", a.label);
    assert_eq!(sessions(&b), ["beta"], "{} got only its own", b.label);

    // The sessions appear in the dashboard's data like any other, with their directories.
    wait("both sessions to be listed", || {
        let s = backend.snapshot(0);
        let has = |label: &str, session: &str| {
            s.hosts
                .iter()
                .any(|h| h.label == label && h.panes.iter().any(|p| p.session == session))
        };
        has("node-a", "alpha") && has("node-b", "beta")
    });
    let snapshot = backend.snapshot(0);
    let pane = |label: &str| {
        snapshot
            .hosts
            .iter()
            .find(|h| h.label == label)
            .and_then(|h| h.panes.first())
            .cloned()
            .expect("pane")
    };
    assert_eq!(format!("{:?}", pane("node-b").agent), "Claude");
    let cwd = |sock: &Sock, session: &str| {
        let out = tmux(
            sock,
            &[
                "display-message",
                "-p",
                "-t",
                &format!("={session}:"),
                "#{pane_current_path}",
            ],
        )
        .unwrap_or_default();
        PathBuf::from(out.trim()).canonicalize().unwrap_or_default()
    };
    assert_eq!(cwd(&a.sock, "alpha"), dir_a);
    assert_eq!(cwd(&b.sock, "beta"), dir_b);

    // Typed refusals come back through the orchestrator, and change nothing.
    assert_eq!(
        request(&mut backend, "node-a", "alpha", &dir_a, Program::Shell),
        Err(CreateFailure::AlreadyExists)
    );
    let missing = scratch.join("missing");
    assert!(matches!(
        request(&mut backend, "node-a", "gamma", &missing, Program::Shell),
        Err(CreateFailure::NoSuchDirectory(_))
    ));
    assert!(!missing.exists(), "no directory is created");
    assert_eq!(sessions(&a), ["alpha"]);

    // A node that is gone is a typed unreachable, not a hang, and nothing is queued for later.
    drop(b.process.take());
    wait("the orchestrator to see node-b go", || {
        host_of(&backend.snapshot(0), "node-b").is_none()
    });
    let host = {
        let s = backend.snapshot(0);
        let h = s
            .hosts
            .iter()
            .find(|h| h.label == "node-b")
            .expect("node-b");
        h.host.clone()
    };
    assert_eq!(
        backend.create_session(&NewSessionRequest {
            host,
            host_label: "node-b".into(),
            name: "late".into(),
            dir: dir_b.to_string_lossy().into_owned(),
            program: Program::Shell,
        }),
        Err(CreateFailure::Unreachable)
    );
    assert!(
        !ssh_log.exists(),
        "ssh was run: {}",
        std::fs::read_to_string(&ssh_log).unwrap_or_default()
    );
}
