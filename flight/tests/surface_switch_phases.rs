// SPDX-License-Identifier: MIT

//! Where the time of a surface switch goes (ADR-009), measured on the library path the
//! dashboard takes, against a real orchestrator and node on a private tmux server. Ignored by
//! default: `cargo test --release -p flight --test surface_switch_phases -- --ignored --nocapture`.

mod support;

use flight_client::{ClientConfig, Handoff, Lease, OrchestratedBackend};
use flight_transport::TerminalClient;
use flight_ui::Backend;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use support::*;

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

#[test]
#[ignore = "measurement, not a pass/fail test"]
fn where_the_time_of_a_switch_goes() {
    let base = PathBuf::from(format!("/tmp/fl-ph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("base");
    let _cleanup = TempDir(base.clone());
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch");
    let claude = fake_claude(&scratch).expect("cc");
    std::fs::write(scratch.join("screen.txt"), PERMIT_SCREEN).expect("screen");
    let (node_cfg, ui_cfg) = (base.join("n"), base.join("u"));
    let (_orch, _addr) = start_orchestrator(&base);
    for (role, name, cfg) in [("node", "mini", &node_cfg), ("ui", "laptop", &ui_cfg)] {
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
    let socket = format!("fl-ph-{}", std::process::id());
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
    .expect("session");
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
            .expect("node"),
    );
    let config = ClientConfig::load(&ui_cfg.join("ui"))
        .or_else(|_| ClientConfig::load(&ui_cfg))
        .expect("config");
    let rounds: usize = std::env::var("FLIGHT_MEASURE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("rt");

    let (
        mut backend_t,
        mut snapshot_t,
        mut reveal_open_t,
        mut stream_t,
        mut lease_t,
        mut first_t,
        mut total_t,
    ) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..rounds {
        let total = Instant::now();
        let t = Instant::now();
        let mut backend = OrchestratedBackend::start(config.clone()).expect("backend");
        backend_t.push(ms(t));
        let t = Instant::now();
        let pane = loop {
            let snap = backend.snapshot(0);
            if let Some(p) = snap.hosts.iter().flat_map(|h| h.panes.iter()).next() {
                break p.clone();
            }
            assert!(t.elapsed() < Duration::from_secs(20), "no pane");
            std::thread::sleep(Duration::from_millis(1));
        };
        snapshot_t.push(ms(t));
        let handoff = backend.handoff();
        let t = Instant::now();
        backend.switch_to(&pane).expect("switch");
        reveal_open_t.push(ms(t));
        let Some(Handoff::Terminal { id, .. }) = handoff.take() else {
            panic!("no terminal")
        };
        let t = Instant::now();
        let client = runtime
            .block_on(TerminalClient::connect_ui(
                &config.address,
                &config.identity,
                &config.orchestrator,
                &id,
            ))
            .expect("stream");
        stream_t.push(ms(t));
        let t = Instant::now();
        let _lease = runtime
            .block_on(Lease::connect(&config, &id))
            .expect("lease");
        lease_t.push(ms(t));
        let t = Instant::now();
        let (_tx, mut rx) = client.split();
        let frame = runtime
            .block_on(async { tokio::time::timeout(Duration::from_secs(10), rx.next()).await });
        assert!(matches!(frame, Ok(Ok(Some(_)))), "no output");
        first_t.push(ms(t));
        total_t.push(ms(total));
        drop(rx);
        drop(backend);
        std::thread::sleep(Duration::from_millis(300));
    }
    let row = |name: &str, v: &Vec<f64>| {
        println!(
            "PHASE {name:<28} p50={:>7.1} ms  max={:>7.1}",
            median(v.clone()),
            v.iter().cloned().fold(0.0, f64::max)
        )
    };
    row("backend start (new runtime)", &backend_t);
    row("until first snapshot", &snapshot_t);
    row("reveal + OpenTerminal", &reveal_open_t);
    row("terminal stream (UI dial)", &stream_t);
    row("lease connection (UI dial)", &lease_t);
    row("first output byte", &first_t);
    row("TOTAL from cold", &total_t);
}
