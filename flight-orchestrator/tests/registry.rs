// SPDX-License-Identifier: MIT

//! Identity, handshake and the node registry.

mod support;

use flight_orchestrator::{ConnId, OrchestratorConfig, OrchestratorCore};
use flight_proto::{
    node_body, orchestrator_body, Heartbeat, NodeFrame, NodeHello, NodeStatusCode, ProtocolVersion,
    CURRENT_VERSION,
};
use flight_state::HostId;
use support::*;

fn hello(node: &str, name: &str, version: ProtocolVersion, caps: &[&str]) -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Hello(NodeHello {
            version: Some(version),
            node_id: node.into(),
            display_name: name.into(),
            capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
            servers: vec!["flight".into()],
        })),
    }
}

fn core() -> OrchestratorCore {
    OrchestratorCore::new(OrchestratorConfig::default(), inc(9))
}

#[test]
fn a_hello_registers_the_node_and_negotiates_capabilities() {
    let mut o = core();
    o.node_connected(ConnId(1), HostId::new("node-a"));
    let fx = o.on_node_frame(
        ConnId(1),
        hello(
            "node-a",
            "mini-1",
            CURRENT_VERSION,
            &["preview", "teleport", "kill"],
        ),
        100,
    );
    let Some(orchestrator_body::Body::Hello(reply)) = fx.to_nodes[0].1.body.clone() else {
        panic!("expected a hello reply")
    };
    assert_eq!(reply.accepted_capabilities, vec!["preview", "kill"]);
    assert_eq!(reply.version, Some(CURRENT_VERSION));
    let snap = o.fleet_snapshot();
    assert_eq!(snap.nodes.len(), 1);
    assert_eq!(snap.nodes[0].status, NodeStatusCode::Online as i32);
}

#[test]
fn a_major_version_mismatch_gets_a_goodbye_and_no_registration() {
    let mut o = core();
    o.node_connected(ConnId(1), HostId::new("node-a"));
    let bad = ProtocolVersion {
        major: CURRENT_VERSION.major + 1,
        minor: 0,
    };
    let fx = o.on_node_frame(ConnId(1), hello("node-a", "x", bad, &[]), 100);
    assert!(matches!(
        fx.to_nodes[0].1.body,
        Some(orchestrator_body::Body::Goodbye(_))
    ));
    assert_eq!(fx.close.len(), 1);
    assert!(o.fleet_snapshot().nodes.is_empty());
}

#[test]
fn a_hello_claiming_another_identity_than_the_authenticated_peer_is_refused() {
    let mut o = core();
    o.node_connected(ConnId(1), HostId::new("node-a"));
    let fx = o.on_node_frame(
        ConnId(1),
        hello("node-b", "liar", CURRENT_VERSION, &[]),
        100,
    );
    assert_eq!(fx.close.len(), 1);
    assert!(o.fleet_snapshot().nodes.is_empty());
}

#[test]
fn the_first_frame_must_be_a_hello() {
    let mut o = core();
    o.node_connected(ConnId(1), HostId::new("node-a"));
    let hb = NodeFrame {
        body: Some(node_body::Body::Heartbeat(Heartbeat { seq: 1 })),
    };
    assert_eq!(o.on_node_frame(ConnId(1), hb, 100).close.len(), 1);
}

#[test]
fn an_invalid_frame_closes_the_connection() {
    let mut o = core();
    o.node_connected(ConnId(1), HostId::new("node-a"));
    assert_eq!(
        o.on_node_frame(ConnId(1), NodeFrame { body: None }, 100)
            .close
            .len(),
        1
    );
}

#[test]
fn two_nodes_with_the_same_display_name_never_collide() {
    let mut w = World::new(vec![
        SimNode::new("node-a", "studio", 1),
        SimNode::new("node-b", "studio", 1),
    ]);
    w.connect(0);
    w.connect(1);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(1, round(1, vec![obs("%1", 2, BUSY_SCREEN)]));
    let views = w.views();
    assert_eq!(views.len(), 2);
    assert_eq!(pane_ids(&views[0]), vec!["%1"]);
    assert_eq!(pane_ids(&views[1]), vec!["%1"]);
    assert_ne!(views[0].panes[0].state, views[1].panes[0].state);
}

#[test]
fn a_display_name_change_keeps_identity_panes_and_routing() {
    let mut w = simple_world();
    w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.subscribe(UiId(1));
    w.disconnect(0);
    w.nodes[0].restart("renamed-mac");
    w.connect(0);
    w.observe(0, round(2, vec![obs("%1", 1, PERMIT_SCREEN)]));
    let v = w.node_view("node-a").expect("same node");
    assert_eq!(v.display_name, "renamed-mac");
    assert_eq!(w.views().len(), 1);
    // The UI mirror followed the rename through a NodeUpsert.
    assert_eq!(w.uis[&UiId(1)].views()[0].display_name, "renamed-mac");
}

#[test]
fn a_newer_connection_supersedes_an_older_one_from_the_same_node() {
    let mut w = simple_world();
    let old = w.connect(0);
    let new = w.connect(0);
    assert_ne!(old, new);
    assert!(w
        .closed
        .iter()
        .any(|(c, why)| *c == old && why.contains("superseded")));
    // Frames still arriving on the old connection are not accepted.
    let fx = w.orch.on_node_frame(
        old,
        NodeFrame {
            body: Some(node_body::Body::Heartbeat(Heartbeat { seq: 1 })),
        },
        w.now,
    );
    assert_eq!(fx.close.len(), 1);
    assert_eq!(w.views().len(), 1);
}

use flight_orchestrator::UiId;
