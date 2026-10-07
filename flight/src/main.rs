// SPDX-License-Identifier: MIT

mod args;
mod config;
mod join_cmd;
mod node_cmd;
mod orchestrator_cmd;
mod roles;
mod ui_cmd;

use config::{Config, ROLES, USAGE};
use flight_control::{HostRegistry, Transport};
use flight_state::{HostId, ServerId};
use flight_tmux::TmuxEndpoint;
use flight_ui::{render_to_string, run, Collector, ViewModel};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("flight: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("orchestrator") => return orchestrator_cmd::run(&args[1..]),
        Some("node") => return node_cmd::run(&args[1..]),
        Some("ui") => return ui_cmd::run_ui(&args[1..]),
        _ => {}
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}\n\n{ROLES}");
        return Ok(());
    }
    let config = Config::parse(args).map_err(|e| format!("{e}\n\n{USAGE}"))?;
    let registry = build_registry(&config)?;
    let mut collector = Collector::new(registry);
    if config.once {
        return once(&mut collector);
    }
    run(collector, config.refresh)
        .map(drop)
        .map_err(|e| e.to_string())
}

fn build_registry(config: &Config) -> Result<HostRegistry, String> {
    let mut r = HostRegistry::new();
    if let Some(socket) = &config.local_socket {
        add(&mut r, "local", Transport::Local, socket)?;
    }
    for t in &config.ssh {
        add(
            &mut r,
            &t.alias,
            Transport::Ssh {
                alias: t.alias.clone(),
            },
            &t.socket,
        )?;
    }
    Ok(r)
}

fn add(r: &mut HostRegistry, host: &str, transport: Transport, socket: &str) -> Result<(), String> {
    let id = HostId::new(host);
    let endpoint = TmuxEndpoint::named(socket).map_err(|e| e.to_string())?;
    r.add_host(id.clone(), transport);
    r.add_server(&id, ServerId::new(socket), endpoint)
        .map_err(|e| e.to_string())
}

/// Collect once and print one frame of the real UI as text.
fn once(collector: &mut Collector) -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut vm = ViewModel::new();
    vm.apply_snapshot(collector.collect(now));
    if let Some(sel) = vm.selected().cloned() {
        vm.apply_preview(Some(collector.preview(&sel)));
    }
    println!("{}", render_to_string(&vm, 110, 32));
    Ok(())
}
