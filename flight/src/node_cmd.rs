// SPDX-License-Identifier: MIT

use crate::args::{config_dir, Args};
use crate::join_cmd::join_command;
use crate::roles::node_dir;
use flight_node::{fresh_incarnation, NodeCore, NodeSession, TmuxServers};
use flight_proto::RoleCode;
use flight_state::ServerId;
use flight_tmux::{SystemRunner, TmuxEndpoint};
use flight_transport::{config_path, identity_dir, run_observer, NodeLink, NodeLinkConfig};
use flight_trust::{ConnectionConfig, Identity};
use std::sync::Arc;
use std::time::Duration;

pub const USAGE: &str = "usage: flight node <command>

  join <bundle> [--name NAME] [--bundle-file PATH] [--config-dir DIR]
        enroll this machine with an orchestrator (the bundle comes from
        `flight orchestrator enrollment create`)
  run [--socket NAME]... [--interval SECS] [--config-dir DIR]
        observe local tmux (tmux -L NAME; default 'flight') and report to the orchestrator";

pub fn run(args: &[String]) -> Result<(), String> {
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
            join_command(&node_dir(&config_dir(&parsed)?), RoleCode::Node, &parsed)
        }
        Some("run") => run_node(&args[1..]),
        _ => Err(USAGE.to_owned()),
    }
}

fn run_node(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &["--socket", "--interval", "--config-dir"], &[])?;
    let dir = node_dir(&config_dir(&args)?);
    let config = ConnectionConfig::load(&config_path(&dir)).map_err(|e| {
        format!("this machine has not joined an orchestrator yet ({e}); run `flight node join`")
    })?;
    let identity = Arc::new(Identity::load(&identity_dir(&dir)).map_err(|e| e.to_string())?);
    let interval = match args.value("--interval") {
        Some(s) => Duration::from_secs(s.parse().map_err(|_| format!("bad --interval {s:?}"))?),
        None => Duration::from_secs(2),
    };

    let mut sockets = args.values("--socket");
    if sockets.is_empty() {
        sockets.push("flight");
    }
    let mut servers = TmuxServers::new();
    for socket in &sockets {
        let endpoint = TmuxEndpoint::named(socket).map_err(|e| e.to_string())?;
        servers.add(
            ServerId::new(*socket),
            Box::new(SystemRunner::new(endpoint)),
        );
    }
    let servers = Arc::new(servers);

    let session = NodeSession::new(
        NodeCore::new(
            identity.fingerprint().host_id(),
            fresh_incarnation().map_err(|e| e.to_string())?,
        ),
        config.display_name.clone(),
    );
    let link = Arc::new(
        NodeLink::new(
            NodeLinkConfig {
                address: config.address.clone(),
                orchestrator: config.orchestrator().map_err(|e| e.to_string())?,
                identity: identity.clone(),
                servers: sockets.iter().map(|s| (*s).to_owned()).collect(),
                heartbeat_interval: Duration::from_secs(5),
                reconnect_min: Duration::from_millis(500),
                reconnect_max: Duration::from_secs(10),
            },
            session,
            servers.clone(),
        )
        .with_log(Arc::new(|line| println!("{line}"))),
    );

    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        println!(
            "flight node {} ({})",
            identity.fingerprint(),
            config.display_name
        );
        println!(
            "reporting to {} (orchestrator {})",
            config.address, config.orchestrator
        );
        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let connection = tokio::spawn({
            let (link, stop_rx) = (link.clone(), stop_rx.clone());
            async move { link.run(stop_rx).await }
        });
        let observer = tokio::spawn(run_observer(link, servers, interval, stop_rx));
        tokio::signal::ctrl_c().await.map_err(|e| e.to_string())?;
        let _ = stop.send(true);
        let _ = connection.await;
        let _ = observer.await;
        Ok(())
    })
}
