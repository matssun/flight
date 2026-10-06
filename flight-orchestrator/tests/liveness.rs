// SPDX-License-Identifier: MIT

//! Liveness is separate from pane state: silence never rewrites panes, and a dropped
//! connection never removes a node.

mod support;

use flight_orchestrator::UiId;
use flight_proto::NodeStatusCode;
use support::*;

fn status(w: &World, node: &str) -> i32 {
    w.node_view(node).expect("node").status
}

fn world_with_panes() -> World {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(1, round(1, vec![obs("%1", 2, BUSY_SCREEN)]));
    w
}

#[test]
fn silence_marks_a_node_stale_then_disconnected_without_touching_its_panes() {
    let mut w = world_with_panes();
    let panes_before = w.node_view("node-a").expect("a").panes;
    w.mute(0);
    for _ in 0..4 {
        w.advance(5);
    }
    assert_eq!(status(&w, "node-a"), NodeStatusCode::Stale as i32);
    assert_eq!(status(&w, "node-b"), NodeStatusCode::Online as i32);
    for _ in 0..4 {
        w.advance(5);
    }
    assert_eq!(status(&w, "node-a"), NodeStatusCode::Disconnected as i32);
    assert_eq!(
        w.node_view("node-a").expect("a").panes,
        panes_before,
        "last-known panes kept exactly as reported"
    );
    assert_eq!(
        w.views().len(),
        2,
        "a node is never removed by a dropped connection"
    );
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}

#[test]
fn a_heartbeat_returns_a_stale_node_to_online() {
    let mut w = world_with_panes();
    w.mute(0);
    w.advance(16);
    assert_eq!(status(&w, "node-a"), NodeStatusCode::Stale as i32);
    w.unmute(0);
    w.advance(1);
    assert_eq!(status(&w, "node-a"), NodeStatusCode::Online as i32);
}

#[test]
fn a_stale_node_keeps_its_replication_stream() {
    let mut w = world_with_panes();
    w.mute(0);
    w.advance(16);
    w.unmute(0);
    // Deltas that arrive while Stale are still in sequence and apply (no resync needed).
    w.observe(0, round(20, vec![obs("%1", 1, BUSY_SCREEN)]));
    assert_eq!(status(&w, "node-a"), NodeStatusCode::Online as i32);
    assert_eq!(
        w.node_view("node-a").expect("a").panes[0].state,
        flight_proto::StateCode::Busy as i32
    );
}

#[test]
fn a_dropped_connection_keeps_the_node_last_known_and_selection_stable() {
    let mut w = world_with_panes();
    let before = w.node_view("node-a").expect("a").panes;
    w.disconnect(0);
    let v = w.node_view("node-a").expect("still listed");
    assert_eq!(v.status, NodeStatusCode::Disconnected as i32);
    assert_eq!(v.panes, before);
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}

#[test]
fn reconnecting_restores_online_and_a_snapshot_replaces_the_last_known_image() {
    let mut w = world_with_panes();
    w.disconnect(0);
    // While away, the node's world changed.
    w.observe(
        0,
        round(
            10,
            vec![obs("%1", 1, BUSY_SCREEN), obs("%2", 3, PERMIT_SCREEN)],
        ),
    );
    w.connect(0);
    let v = w.node_view("node-a").expect("a");
    assert_eq!(v.status, NodeStatusCode::Online as i32);
    assert_eq!(pane_ids(&v), vec!["%1", "%2"]);
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}
