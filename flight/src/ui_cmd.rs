// SPDX-License-Identifier: MIT

use crate::args::{config_dir, now, Args};
use crate::join_cmd::join_command;
use crate::roles::{node_dir, ui_dir};
use flight_client::{run_terminal, ClientConfig, Handoff, OrchestratedBackend, Switcher};
use flight_proto::RoleCode;
use flight_state::HostId;
use flight_ui::{render_to_string, run_with_notice, Backend, Exit, ViewModel};
use std::time::Duration;

pub const USAGE: &str = "usage: flight ui <command>

  join <bundle> [--name NAME] [--bundle-file PATH] [--config-dir DIR]
        enroll this machine's UI with an orchestrator
  [run] [--refresh SECS] [--once] [--config-dir DIR] [--node-dir DIR]
        the dashboard, reading from the orchestrator this UI joined.
        Enter on a pane selects it and shows it. A pane of the node on this machine (its
        identity is read from --node-dir, default <config-dir>/node) is shown through tmux. A
        pane of any other node is shown in a terminal carried over Flight's own connections
        (no ssh, no direct path to the node): Ctrl-] q leaves, Ctrl-] Ctrl-] sends a literal
        Ctrl-], and the dashboard comes back when the terminal ends.";

pub fn run_ui(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("join") => {
            let parsed = Args::parse(
                &args[1..],
                &["--name", "--bundle-file", "--config-dir"],
                &[],
            )?;
            join_command(&ui_dir(&config_dir(&parsed)?), RoleCode::Ui, &parsed)
        }
        Some("run") => dashboard(&args[1..]),
        _ => dashboard(args),
    }
}

fn dashboard(args: &[String]) -> Result<(), String> {
    let args = Args::parse(
        args,
        &["--refresh", "--config-dir", "--node-dir"],
        &["--once"],
    )?;
    let base = config_dir(&args)?;
    let dir = ui_dir(&base);
    let config = ClientConfig::load(&dir).map_err(|e| {
        format!("this UI has not joined an orchestrator yet ({e}); run `flight ui join`")
    })?;
    let refresh = match args.value("--refresh") {
        Some(s) => Duration::from_secs(s.parse().map_err(|_| format!("bad --refresh {s:?}"))?),
        None => Duration::from_secs(1),
    };
    let node = args
        .value("--node-dir")
        .map_or_else(|| node_dir(&base), std::path::PathBuf::from);
    let switcher = Switcher::new(local_host(&node));
    let start = |config: ClientConfig| -> Result<OrchestratedBackend, String> {
        let mut backend = OrchestratedBackend::start(config).map_err(|e| e.to_string())?;
        backend.set_switching(switcher.clone());
        Ok(backend)
    };
    if args.switch("--once") {
        return once(start(config)?);
    }
    // A remote pane is shown in a terminal over Flight; when it ends the dashboard comes
    // back, with the reason on its status line.
    let mut notice = None;
    loop {
        let backend = start(config.clone())?;
        let handoff = backend.handoff();
        let exit = run_with_notice(backend, refresh, notice.take()).map_err(|e| e.to_string())?;
        if exit != Exit::Switched {
            return Ok(());
        }
        match handoff.take() {
            Some(Handoff::Attach(attach)) => {
                // Only returns if the program could not be started: the pane is already selected.
                let why = attach.exec();
                return Err(format!("pane selected, but cannot attach: {why}"));
            }
            Some(Handoff::Terminal(id)) => {
                let end = run_terminal(&config, &id);
                notice = Some(format!("terminal: {end}"));
            }
            None => return Ok(()),
        }
    }
}

/// The identity of the node role on this machine, if it has one.
fn local_host(node_dir: &std::path::Path) -> Option<HostId> {
    flight_trust::Identity::load(&flight_transport::identity_dir(node_dir))
        .ok()
        .map(|i| i.fingerprint().host_id())
}

/// Wait briefly for the orchestrator's snapshot, then print one frame as text.
fn once(mut backend: OrchestratedBackend) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !backend.connected() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut vm = ViewModel::new();
    vm.apply_snapshot(backend.snapshot(now()));
    if let Some(sel) = vm.selected().cloned() {
        vm.apply_preview(Some(backend.preview(&sel)));
    }
    println!("{}", render_to_string(&vm, 110, 32));
    Ok(())
}
