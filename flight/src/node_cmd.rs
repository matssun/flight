// SPDX-License-Identifier: MIT

use crate::args::{config_dir, Args};
use crate::join_cmd::join_command;
use crate::roles::node_dir;
use flight_node::{fresh_incarnation, NodeCore, NodeSession, TmuxServers};
use flight_proto::RoleCode;
use flight_state::ServerId;
use flight_tmux::{SystemRunner, TmuxEndpoint};
use flight_transport::{
    config_path, identity_dir, run_observer, LinkEnd, NodeLink, NodeLinkConfig,
};
use flight_trust::{ConnectionConfig, Identity};
use std::sync::Arc;
use std::time::Duration;

pub const USAGE: &str = "usage: flight node <command>

  join <bundle> [--name NAME] [--bundle-file PATH] [--config-dir DIR]
        enroll this machine with an orchestrator (the bundle comes from
        `flight orchestrator enrollment create`)
  run [--socket NAME]... [--interval SECS] [--exit-after-link-down SECS] [--config-dir DIR]
        observe local tmux (tmux -L NAME; default 'flight') and report to the orchestrator.
        --exit-after-link-down: exit with status 75 after this long of nothing but immediate
        \"no route to host\" failures (never because the orchestrator is merely down), so that a
        supervisor (launchd, systemd, a shell loop) restarts the node. Restarting is safe: tmux
        and the agents are untouched and the node resynchronises with a full snapshot. Needed on
        macOS, where a long-running process can stay unable to reach a peer after a Wi-Fi bounce.";

/// Exit status of a node that gave up because its own process cannot use the network
/// (EX_TEMPFAIL): "restart me".
pub const EXIT_PROCESS_NETWORK_UNHEALTHY: i32 = 75;

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
    let args = Args::parse(
        args,
        &[
            "--socket",
            "--interval",
            "--exit-after-link-down",
            "--config-dir",
        ],
        &[],
    )?;
    let dir = node_dir(&config_dir(&args)?);
    let config = ConnectionConfig::load(&config_path(&dir)).map_err(|e| {
        format!("this machine has not joined an orchestrator yet ({e}); run `flight node join`")
    })?;
    let identity = Arc::new(Identity::load(&identity_dir(&dir)).map_err(|e| e.to_string())?);
    let interval = match args.value("--interval") {
        Some(s) => parse_interval(s)?,
        None => Duration::from_secs(2),
    };

    let exit_after = match args.value("--exit-after-link-down") {
        Some(s) => match s
            .parse::<u64>()
            .map_err(|_| format!("bad --exit-after-link-down {s:?}"))?
        {
            0 => None,
            secs => Some(Duration::from_secs(secs)),
        },
        None => None,
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
    let link = NodeLink::new(
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
    .with_log(Arc::new(|line| {
        println!("{} {line}", crate::clock::utc_clock());
    }));
    let link = Arc::new(match exit_after {
        Some(limit) => link.with_unreachable_limit(limit),
        None => link,
    });

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
        let mut connection = connection;
        let ended = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|e| e.to_string())?;
                None
            }
            ended = &mut connection => Some(ended),
        };
        let _ = stop.send(true);
        let unhealthy = match ended {
            Some(result) => matches!(result, Ok(LinkEnd::ProcessNetworkUnhealthy)),
            None => {
                let _ = connection.await;
                false
            }
        };
        let _ = observer.await;
        if unhealthy {
            std::process::exit(EXIT_PROCESS_NETWORK_UNHEALTHY);
        }
        Ok(())
    })
}

/// A poll interval in (possibly fractional) seconds, at least 50 ms and at most an hour.
fn parse_interval(s: &str) -> Result<Duration, String> {
    let bad = || format!("bad --interval {s:?}: seconds between 0.05 and 3600, e.g. 2 or 0.5");
    let secs: f64 = s.parse().map_err(|_| bad())?;
    if !(0.05..=3600.0).contains(&secs) {
        return Err(bad());
    }
    Duration::try_from_secs_f64(secs).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::parse_interval;
    use std::time::Duration;

    #[test]
    fn intervals_may_be_fractional_but_must_be_sane() {
        assert_eq!(parse_interval("2"), Ok(Duration::from_secs(2)));
        assert_eq!(parse_interval("0.25"), Ok(Duration::from_millis(250)));
        for bad in ["0", "0.01", "-1", "nan", "inf", "7200", "abc", ""] {
            assert!(parse_interval(bad).is_err(), "{bad:?}");
        }
    }
}
