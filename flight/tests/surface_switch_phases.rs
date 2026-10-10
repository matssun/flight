// SPDX-License-Identifier: MIT

//! Where the time of a surface switch goes (ADR-009), measured on the library path the
//! dashboard takes, against a real orchestrator and node on a private tmux server. Ignored by
//! default: `cargo test --release -p flight --test surface_switch_phases -- --ignored --nocapture`.

mod support;

use flight_client::{
    ClientConfig, Handoff, LinkHost, OpenRequest, OrchestratedBackend, SurfaceHost,
};
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
    // The link is started once, as the dashboard process now does.
    let t = Instant::now();
    let mut backend = OrchestratedBackend::start(config.clone()).expect("backend");
    let pane = loop {
        let snap = backend.snapshot(0);
        if let Some(p) = snap.hosts.iter().flat_map(|h| h.panes.iter()).next() {
            break p.clone();
        }
        assert!(t.elapsed() < Duration::from_secs(20), "no pane");
        std::thread::sleep(Duration::from_millis(1));
    };
    println!(
        "PHASE link start until first snapshot (once) {:.1} ms",
        ms(t)
    );
    let workspace = flight_ui::WorkspaceKey {
        host: pane.pane_ref.host.clone(),
        workspace: pane.workspace.clone(),
    };
    let host = LinkHost::new(backend.clone(), workspace);
    let (mut dash_open_t, mut dash_stream_t, mut dash_lease_t, mut dash_total_t) =
        (vec![], vec![], vec![], vec![]);
    let (mut session_open_t, mut session_first_t, mut session_total_t) = (vec![], vec![], vec![]);
    for _ in 0..rounds {
        // The path a switch took before the surface session: reveal and open through the
        // dashboard backend, then two new connections (the terminal stream and the lease).
        let total = Instant::now();
        let handoff = backend.handoff();
        let t = Instant::now();
        backend.switch_to(&pane).expect("switch");
        dash_open_t.push(ms(t));
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
        dash_stream_t.push(ms(t));
        let t = Instant::now();
        let control = runtime
            .block_on(flight_transport::UiClient::connect(
                &config.address,
                &config.identity,
                &config.orchestrator,
            ))
            .expect("lease connection");
        dash_lease_t.push(ms(t));
        drop(control);
        dash_total_t.push(ms(total));
        drop(client);
        std::thread::sleep(Duration::from_millis(300));

        // The path now: the link already exists; ask for the terminal and connect.
        let total = Instant::now();
        let t = Instant::now();
        let mut attachment = runtime
            .block_on(host.open(OpenRequest {
                choice: flight_ui::SurfaceChoice::Agent,
                surface: None,
                cols: 100,
                rows: 30,
                expect: None,
            }))
            .expect("open");
        session_open_t.push(ms(t));
        let t = Instant::now();
        let frame = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), attachment.from_remote.recv()).await
        });
        assert!(matches!(frame, Ok(Some(_))), "no output");
        session_first_t.push(ms(t));
        session_total_t.push(ms(total));
        drop(attachment);
        std::thread::sleep(Duration::from_millis(300));
    }
    let row = |name: &str, v: &Vec<f64>| {
        println!(
            "PHASE {name:<30} p50={:>7.1} ms  max={:>7.1}",
            median(v.clone()),
            v.iter().cloned().fold(0.0, f64::max)
        )
    };
    println!("-- before: reveal and open through the dashboard, then new connections");
    row("reveal + OpenTerminal", &dash_open_t);
    row("terminal stream (UI dial)", &dash_stream_t);
    row("lease connection (UI dial)", &dash_lease_t);
    row("TOTAL (after the dashboard)", &dash_total_t);
    println!("-- now: the surface session over the link that is already up");
    row("OpenTerminal + stream", &session_open_t);
    row("first output byte", &session_first_t);
    row("TOTAL", &session_total_t);
}
