// SPDX-License-Identifier: MIT

//! Per-node replication: transactional images, gaps, incarnations, restarts.

mod support;

use flight_orchestrator::{ConnId, UiId};
use flight_proto::{
    delta_change, node_body, orchestrator_body, Delta, NodeFrame, PaneRefMsg, Snapshot, StateCode,
    Step,
};
use support::*;

fn delta_frame(incarnation: u8, sequence: u64, change: delta_change::Change) -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Delta(Delta {
            incarnation: inc(incarnation).as_bytes().to_vec(),
            sequence,
            change: Some(change),
        })),
    }
}

fn state_of(w: &World, node: &str, pane: &str) -> Option<i32> {
    w.node_view(node)?
        .panes
        .iter()
        .find(|p| p.pane_ref.as_ref().is_some_and(|r| r.pane == pane))
        .map(|p| p.state)
}

#[test]
fn snapshot_then_deltas_reach_the_fleet_image_and_the_ui() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(0, round(2, vec![obs("%1", 1, BUSY_SCREEN)]));
    assert_eq!(state_of(&w, "node-a", "%1"), Some(StateCode::Busy as i32));
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}

#[test]
fn a_gap_keeps_the_last_consistent_image_and_asks_once_for_a_snapshot() {
    let mut w = simple_world();
    let conn = w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    let before = w.node_view("node-a");
    // Lose one delta in transit, then deliver later ones by hand.
    w.drop_node_frames = true;
    w.observe(0, round(2, vec![obs("%1", 1, BUSY_SCREEN)]));
    w.drop_node_frames = false;
    let frames = w.nodes[0].session.observe(vec![round(
        3,
        vec![obs("%1", 1, PERMIT_SCREEN), obs("%2", 2, BUSY_SCREEN)],
    )]);
    assert!(frames.len() >= 2);
    // The first out-of-sequence delta triggers exactly one resync request, and the image is
    // untouched by any of them.
    let mut asked = 0;
    for f in frames {
        let fx = w.orch.on_node_frame(conn, f, w.now);
        asked += fx
            .to_nodes
            .iter()
            .filter(|(_, o)| matches!(o.body, Some(orchestrator_body::Body::Resync(_))))
            .count();
        assert_eq!(
            w.node_view("node-a"),
            before,
            "image changed by a rejected delta"
        );
    }
    assert_eq!(asked, 1);
}

#[test]
fn a_snapshot_after_a_gap_repairs_the_image() {
    let mut w = simple_world();
    let conn = w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.drop_node_frames = true;
    w.observe(0, round(2, vec![obs("%1", 1, BUSY_SCREEN)]));
    w.drop_node_frames = false;
    w.observe(
        0,
        round(
            3,
            vec![obs("%1", 1, BUSY_SCREEN), obs("%2", 2, PERMIT_SCREEN)],
        ),
    );
    // The harness answers the resync request with a fresh snapshot.
    assert_eq!(state_of(&w, "node-a", "%1"), Some(StateCode::Busy as i32));
    assert!(state_of(&w, "node-a", "%2").is_some());
    let _ = conn;
}

#[test]
fn a_gap_on_one_node_does_not_disturb_another() {
    let mut w = simple_world();
    w.connect(0);
    w.connect(1);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(1, round(1, vec![obs("%1", 2, PERMIT_SCREEN)]));
    w.drop_node_frames = true;
    w.observe(0, round(2, vec![obs("%1", 1, BUSY_SCREEN)]));
    w.drop_node_frames = false;
    // Node B keeps flowing normally while node A needs a resync.
    w.observe(1, round(3, vec![obs("%1", 2, BUSY_SCREEN)]));
    assert_eq!(state_of(&w, "node-b", "%1"), Some(StateCode::Busy as i32));
}

#[test]
fn a_reconnect_with_the_same_incarnation_needs_a_snapshot_before_deltas() {
    let mut w = simple_world();
    w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.disconnect(0);
    // A continuation delta from the old stream arrives on the new connection before any snapshot.
    let conn = ConnId(99);
    w.orch
        .node_connected(conn, flight_state::HostId::new("node-a"));
    let hello = w.nodes[0].session.connect(vec![]);
    w.orch.on_node_frame(conn, hello, w.now);
    let stale = delta_frame(
        1,
        2,
        delta_change::Change::PaneRemoved(PaneRefMsg {
            host: "node-a".into(),
            server: "flight".into(),
            pane: "%1".into(),
        }),
    );
    let fx = w.orch.on_node_frame(conn, stale, w.now);
    assert!(fx
        .to_nodes
        .iter()
        .any(|(_, o)| matches!(o.body, Some(orchestrator_body::Body::Resync(_)))));
    assert!(
        state_of(&w, "node-a", "%1").is_some(),
        "the removal was not applied"
    );
}

#[test]
fn a_crashed_node_with_a_new_incarnation_cannot_continue_the_old_stream() {
    let mut w = simple_world();
    w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.disconnect(0);
    w.nodes[0].restart("mini-1");
    w.connect(0);
    // New process, cold start: its snapshot replaces the image; old-incarnation deltas are refused.
    w.observe(0, round(5, vec![obs("%1", 1, IDLE_SCREEN)]));
    assert_eq!(state_of(&w, "node-a", "%1"), Some(StateCode::Idle as i32));
    let conn = w.nodes[0].conn.expect("connected");
    let old = delta_frame(
        1,
        1,
        delta_change::Change::PaneRemoved(PaneRefMsg {
            host: "node-a".into(),
            server: "flight".into(),
            pane: "%1".into(),
        }),
    );
    w.orch.on_node_frame(conn, old, w.now);
    assert!(state_of(&w, "node-a", "%1").is_some());
}

#[test]
fn a_pane_claiming_another_nodes_identity_is_a_violation() {
    let mut w = simple_world();
    let conn = w.connect(0);
    // Node B observes a pane (it need not be connected); node A will forge a snapshot from it.
    w.observe(1, round(1, vec![obs("%1", 2, PERMIT_SCREEN)]));
    let stolen = w.nodes[1].session.core().state().panes[0].clone();
    let forged = NodeFrame {
        body: Some(node_body::Body::Snapshot(Snapshot {
            incarnation: inc(1).as_bytes().to_vec(),
            panes: vec![stolen],
            servers: vec![],
        })),
    };
    let fx = w.orch.on_node_frame(conn, forged, w.now);
    assert_eq!(fx.close.len(), 1);
    assert!(w.node_view("node-a").expect("node").panes.is_empty());
}

#[test]
fn an_orchestrator_restart_gets_a_new_incarnation_and_nodes_rebuild_it() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(1, round(1, vec![obs("%1", 2, BUSY_SCREEN)]));
    let before = w.snapshot();
    w.restart_orchestrator();
    assert_ne!(w.snapshot().incarnation, before.incarnation);
    assert!(w.views().is_empty(), "nothing durable survives");
    w.connect(0);
    w.connect(1);
    assert_eq!(w.views(), before.nodes);
    assert_eq!(w.uis[&UiId(1)].views(), before.nodes);
}

#[test]
fn old_ui_deltas_cannot_continue_into_a_new_orchestrator_incarnation() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    let mut old_mirror = w.uis.remove(&UiId(1)).expect("mirror");
    assert!(old_mirror.in_sync());
    w.restart_orchestrator();
    // A delta of the new orchestrator incarnation, even with a plausible sequence number,
    // is never applied to a stream that belongs to the old one.
    let probe = flight_proto::UiEvent {
        body: Some(flight_proto::ui_event_body::Body::Delta(
            flight_proto::FleetDelta {
                incarnation: w.orch.incarnation().as_bytes().to_vec(),
                sequence: 1,
                change: Some(flight_proto::fleet_change::Change::NodeRemoved(
                    flight_proto::NodeRemoved {
                        node_id: "node-a".into(),
                    },
                )),
            },
        )),
    };
    assert_eq!(old_mirror.apply(&probe), Step::Resync);
}
