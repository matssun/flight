// SPDX-License-Identifier: MIT

use crate::args::{config_dir, default_name, Args};
use crate::roles::orchestrator_dir;
use flight_node::fresh_incarnation;
use flight_orchestrator::OrchestratorConfig;
use flight_transport::{admin_request, identity_dir, serve, serve_admin, ServerConfig};
use flight_trust::{Identity, TrustStore};
use std::time::Duration;

pub const USAGE: &str = "usage: flight orchestrator <command>

  run [--listen ADDR] [--advertise ADDR] [--name NAME] [--config-dir DIR]
        serve this machine as the orchestrator (default listen 127.0.0.1:7676; for a LAN,
        --listen 0.0.0.0:7676 --advertise <this machine's address>:7676)
  enrollment create [--ttl SECS] [--config-dir DIR]
        print a one-time bundle for `flight node join` / `flight ui join`
  trust list | revoke <id> | status [--config-dir DIR]
        inspect or revoke the identities this orchestrator trusts";

pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("run") => serve_command(&args[1..]),
        Some("enrollment") if args.get(1).map(String::as_str) == Some("create") => {
            enrollment_create(&args[2..])
        }
        Some("trust") => trust(&args[1..]),
        _ => Err(USAGE.to_owned()),
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Runtime::new().map_err(|e| e.to_string())
}

fn serve_command(args: &[String]) -> Result<(), String> {
    let args = Args::parse(
        args,
        &["--listen", "--advertise", "--name", "--config-dir"],
        &[],
    )?;
    let dir = orchestrator_dir(&config_dir(&args)?);
    let identity = Identity::load_or_create(&identity_dir(&dir)).map_err(|e| e.to_string())?;
    let fingerprint = identity.fingerprint().clone();

    let trust_path = dir.join("trust.toml");
    let mut trust = TrustStore::load(&trust_path).map_err(|e| e.to_string())?;
    let name = args
        .value("--name")
        .map_or_else(default_name, str::to_owned);
    trust.set_orchestrator(&fingerprint, &name);
    trust.save(&trust_path).map_err(|e| e.to_string())?;

    let bind = args.value("--listen").unwrap_or("127.0.0.1:7676");
    let rt = runtime()?;
    rt.block_on(async {
        let handle = serve(ServerConfig {
            bind: bind
                .parse()
                .map_err(|e| format!("bad --listen {bind:?}: {e}"))?,
            identity,
            trust,
            trust_path: Some(trust_path),
            core: OrchestratorConfig::default(),
            incarnation: fresh_incarnation().map_err(|e| e.to_string())?,
            tick_interval: Duration::from_secs(1),
        })
        .await
        .map_err(|e| e.to_string())?;
        let advertise = args
            .value("--advertise")
            .map_or_else(|| handle.local_addr().to_string(), str::to_owned);
        let admin = serve_admin(
            dir.join("admin.sock"),
            handle.clone_control(),
            advertise.clone(),
        )
        .map_err(|e| e.to_string())?;
        println!("flight orchestrator {fingerprint}");
        println!("listening on {}", handle.local_addr());
        println!("advertising {advertise}");
        tokio::signal::ctrl_c().await.map_err(|e| e.to_string())?;
        admin.abort();
        handle.shutdown().await;
        Ok(())
    })
}

fn enrollment_create(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &["--ttl", "--config-dir"], &[])?;
    let dir = orchestrator_dir(&config_dir(&args)?);
    let request = match args.value("--ttl") {
        Some(ttl) => format!("enroll {ttl}"),
        None => "enroll".to_owned(),
    };
    let bundle = runtime()?
        .block_on(admin_request(&dir.join("admin.sock"), &request))
        .map_err(|e| e.to_string())?;
    println!("{bundle}");
    Ok(())
}

fn trust(args: &[String]) -> Result<(), String> {
    let (sub, rest) = args.split_first().ok_or_else(|| USAGE.to_owned())?;
    let parsed = Args::parse(rest, &["--config-dir"], &[])?;
    let dir = orchestrator_dir(&config_dir(&parsed)?);
    let request = match (sub.as_str(), parsed.positional.first()) {
        ("list", _) => "trust".to_owned(),
        ("status", _) => "status".to_owned(),
        ("revoke", Some(id)) => format!("revoke {id}"),
        _ => return Err(USAGE.to_owned()),
    };
    let reply = runtime()?
        .block_on(admin_request(&dir.join("admin.sock"), &request))
        .map_err(|e| e.to_string())?;
    if !reply.is_empty() {
        println!("{reply}");
    }
    Ok(())
}
