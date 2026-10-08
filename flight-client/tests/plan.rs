// SPDX-License-Identifier: MIT

//! `plan_switch` is a pure function of the target and three facts about this machine.

use flight_client::{
    plan_switch, Refusal, SshDestinations, SwitchPlan, SwitchTarget, UiContext, UiPlacement,
};
use flight_state::{HostId, PaneId, PaneRef, ServerId};

const LOCAL: &str = "local-node-id";
const REMOTE: &str = "remote-node-id";

fn target(host: &str, pane: &str, pid: u32) -> SwitchTarget {
    SwitchTarget {
        pane: PaneRef {
            host: HostId::new(host),
            server: ServerId::new("flight"),
            pane: PaneId::new(pane),
        },
        pid,
        session: "api".to_owned(),
    }
}

fn inside(server: Option<&str>, clients: &[&str]) -> UiPlacement {
    UiPlacement::InsideTmux {
        server: server.map(str::to_owned),
        clients: clients.iter().map(|c| (*c).to_owned()).collect(),
    }
}

fn plan(
    t: &SwitchTarget,
    local: Option<&str>,
    placement: &UiPlacement,
    ssh: &SshDestinations,
) -> Result<SwitchPlan, Refusal> {
    let local = local.map(HostId::new);
    plan_switch(
        t,
        &UiContext {
            local_host: local.as_ref(),
            placement,
            ssh,
        },
    )
}

fn no_ssh() -> SshDestinations {
    SshDestinations::default()
}

#[test]
fn one_client_on_the_dashboards_session_is_moved() {
    let t = target(LOCAL, "%3", 10);
    assert_eq!(
        plan(
            &t,
            Some(LOCAL),
            &inside(Some("flight"), &["/dev/ttys004"]),
            &no_ssh()
        ),
        Ok(SwitchPlan::LocalClient {
            client: "/dev/ttys004".to_owned(),
            target: t
        })
    );
}

#[test]
fn zero_or_several_clients_refuse() {
    let t = target(LOCAL, "%3", 10);
    assert_eq!(
        plan(&t, Some(LOCAL), &inside(Some("flight"), &[]), &no_ssh()),
        Err(Refusal::NoClient)
    );
    assert_eq!(
        plan(
            &t,
            Some(LOCAL),
            &inside(Some("flight"), &["a", "b"]),
            &no_ssh()
        ),
        Err(Refusal::AmbiguousClients(2))
    );
}

#[test]
fn a_dashboard_in_another_tmux_server_is_never_nested_into() {
    let t = target(LOCAL, "%3", 10);
    for server in [Some("work"), None] {
        assert!(matches!(
            plan(&t, Some(LOCAL), &inside(server, &["a"]), &no_ssh()),
            Err(Refusal::OtherTmuxServer { .. })
        ));
    }
}

#[test]
fn a_dashboard_outside_tmux_attaches_to_a_local_pane() {
    let t = target(LOCAL, "%3", 10);
    assert_eq!(
        plan(&t, Some(LOCAL), &UiPlacement::OutsideTmux, &no_ssh()),
        Ok(SwitchPlan::LocalAttach {
            server: "flight".to_owned(),
            target: t
        })
    );
}

#[test]
fn a_remote_pane_needs_a_destination_keyed_by_its_node_id() {
    let t = target(REMOTE, "%3", 10);
    assert_eq!(
        plan(&t, Some(LOCAL), &UiPlacement::OutsideTmux, &no_ssh()),
        Err(Refusal::NoSshDestination(HostId::new(REMOTE)))
    );
    // A destination for some other node, or one keyed by a name, is not this node's.
    let other = SshDestinations::parse(
        "[nodes.\"someone-else\"]\nssh = \"x\"\n[nodes.\"mini\"]\nssh = \"y\"\nname = \"remote-node-id\"\n",
    )
    .unwrap();
    assert_eq!(
        plan(&t, Some(LOCAL), &UiPlacement::OutsideTmux, &other),
        Err(Refusal::NoSshDestination(HostId::new(REMOTE)))
    );
    let ssh = SshDestinations::parse("[nodes.\"remote-node-id\"]\nssh = \"mini-2\"\n").unwrap();
    assert_eq!(
        plan(&t, Some(LOCAL), &UiPlacement::OutsideTmux, &ssh),
        Ok(SwitchPlan::RemoteAttach {
            host: HostId::new(REMOTE),
            ssh_alias: "mini-2".to_owned(),
            server: "flight".to_owned(),
            target: t
        })
    );
}

#[test]
fn a_remote_pane_does_not_depend_on_where_the_dashboard_runs() {
    let t = target(REMOTE, "%3", 10);
    let ssh = SshDestinations::parse("[nodes.\"remote-node-id\"]\nssh = \"mini-2\"\n").unwrap();
    for placement in [
        UiPlacement::OutsideTmux,
        inside(Some("flight"), &["a", "b"]),
    ] {
        assert!(matches!(
            plan(&t, Some(LOCAL), &placement, &ssh),
            Ok(SwitchPlan::RemoteAttach { .. })
        ));
    }
}

#[test]
fn with_no_node_on_this_machine_every_pane_is_remote() {
    // A coincidentally equal pane id on another machine is not local.
    let t = target(LOCAL, "%3", 10);
    assert_eq!(
        plan(&t, None, &UiPlacement::OutsideTmux, &no_ssh()),
        Err(Refusal::NoSshDestination(HostId::new(LOCAL)))
    );
}

#[test]
fn a_pane_without_a_known_process_or_a_sane_name_is_refused() {
    let ssh = no_ssh();
    let out = UiPlacement::OutsideTmux;
    assert_eq!(
        plan(&target(LOCAL, "%3", 0), Some(LOCAL), &out, &ssh),
        Err(Refusal::UnknownProcess)
    );
    for bad in ["", "3", "%", "%x", "%1;rm", "%-1", "-t"] {
        assert_eq!(
            plan(&target(LOCAL, bad, 5), Some(LOCAL), &out, &ssh),
            Err(Refusal::BadTarget("pane id")),
            "{bad:?}"
        );
    }
    let mut t = target(LOCAL, "%3", 5);
    t.pane.server = ServerId::new("-x");
    assert_eq!(
        plan(&t, Some(LOCAL), &out, &ssh),
        Err(Refusal::BadTarget("tmux server name"))
    );
}
