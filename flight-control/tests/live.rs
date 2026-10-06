// SPDX-License-Identifier: MIT

//! Real tmux on a private socket, and a real (failing) ssh invocation. Never touches the
//! default tmux server. Skipped when tmux or ssh is absent.

use flight_control::{HostError, HostRegistry, Transport};
use flight_state::{HostId, ServerId};
use flight_tmux::TmuxEndpoint;
use std::process::Command;

fn have(bin: &str, flag: &str) -> bool {
    Command::new(bin).arg(flag).output().is_ok()
}

#[test]
fn local_host_lifecycle_through_the_registry() {
    if !have("tmux", "-V") {
        return;
    }
    let (host, server) = (HostId::new("local"), ServerId::new("flight-test"));
    let sock = format!("flight-control-test-{}", std::process::id());
    let mut r = HostRegistry::new();
    r.add_host(host.clone(), Transport::Local);
    r.add_server(&host, server.clone(), TmuxEndpoint::named(&sock).unwrap())
        .unwrap();

    // tmux present, no server yet: a distinct state from "unreachable".
    let before = r.status(&host, &server).unwrap();
    assert_eq!(
        (
            before.reachable,
            before.tmux_available,
            before.endpoint_available
        ),
        (true, true, false)
    );
    assert_eq!(before.problem, Some(HostError::TmuxServerUnavailable));
    assert_eq!(
        r.list_panes(&host, &server),
        Err(HostError::TmuxServerUnavailable)
    );

    r.new_session(&host, &server, "api", "/tmp").unwrap();
    assert!(r.status(&host, &server).unwrap().is_online());
    let panes = r.list_panes(&host, &server).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane_ref.host, host);
    assert!(r.capture_pane(&panes[0].pane_ref, false, Some(5)).is_ok());

    r.kill_server(&host, &server).unwrap();
    assert_eq!(
        r.list_panes(&host, &server),
        Err(HostError::TmuxServerUnavailable)
    );
}

#[test]
fn a_real_ssh_to_a_nonexistent_host_is_reported_unreachable() {
    if !have("ssh", "-V") {
        return;
    }
    let (host, server) = (HostId::new("ghost"), ServerId::new("flight"));
    let mut r = HostRegistry::new();
    r.add_host(
        host.clone(),
        Transport::Ssh {
            alias: "flight-no-such-host.invalid".into(),
        },
    );
    r.add_server(
        &host,
        server.clone(),
        TmuxEndpoint::named("flight").unwrap(),
    )
    .unwrap();
    let status = r.status(&host, &server).unwrap();
    assert!(!status.reachable);
    assert!(
        matches!(status.problem, Some(HostError::HostUnreachable { .. })),
        "{:?}",
        status.problem
    );
}

/// Opt-in: set FLIGHT_TEST_SSH_ALIAS to an ssh alias with tmux installed to exercise the
/// real SSH path end to end (a private socket on that host, killed afterwards).
#[test]
fn optional_real_ssh_host_lifecycle() {
    let Ok(alias) = std::env::var("FLIGHT_TEST_SSH_ALIAS") else {
        return;
    };
    let (host, server) = (HostId::new("remote"), ServerId::new("flight-test"));
    let sock = format!("flight-control-ssh-test-{}", std::process::id());
    let mut r = HostRegistry::new();
    r.add_host(host.clone(), Transport::Ssh { alias });
    r.add_server(&host, server.clone(), TmuxEndpoint::named(&sock).unwrap())
        .unwrap();
    r.new_session(&host, &server, "api", "/tmp").unwrap();
    assert!(r.status(&host, &server).unwrap().is_online());
    assert_eq!(r.list_panes(&host, &server).unwrap().len(), 1);
    r.kill_server(&host, &server).unwrap();
}
