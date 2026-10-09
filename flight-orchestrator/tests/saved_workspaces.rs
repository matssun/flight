// SPDX-License-Identifier: MIT

//! A node's saved workspaces reach the UI through the orchestrator, survive a snapshot, and
//! are only sent to an orchestrator that accepted the capability.

mod support;

use flight_orchestrator::UiId;
use flight_proto::{capability, SavedHealthCode};
use support::*;

#[test]
fn a_report_reaches_the_fleet_image_and_the_subscribed_ui() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.observe_saved(0, vec![saved("c-1", "nga", SavedHealthCode::Stopped)]);
    assert_eq!(w.node_view("node-a").unwrap().saved.len(), 1);
    assert_eq!(
        w.uis[&UiId(1)].views(),
        w.views(),
        "the UI mirrors the orchestrator"
    );
    // A new report replaces the whole list.
    w.observe_saved(
        0,
        vec![
            saved("c-1", "nga", SavedHealthCode::Running),
            saved("c-2", "api", SavedHealthCode::Blocked),
        ],
    );
    let view = w.node_view("node-a").unwrap();
    assert_eq!(view.saved.len(), 2);
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
    w.observe_saved(0, vec![]);
    assert!(w.node_view("node-a").unwrap().saved.is_empty());
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}

#[test]
fn an_unchanged_report_sends_nothing() {
    let mut w = simple_world();
    w.connect(0);
    let list = vec![saved("c-1", "nga", SavedHealthCode::Stopped)];
    w.observe_saved(0, list.clone());
    assert!(w.nodes[0].session.observe_saved(list).is_empty());
}

#[test]
fn a_node_that_reports_before_connecting_is_in_the_first_snapshot() {
    let mut w = simple_world();
    w.observe_saved(0, vec![saved("c-1", "nga", SavedHealthCode::Stopped)]);
    w.connect(0);
    assert_eq!(w.node_view("node-a").unwrap().saved.len(), 1);
}

#[test]
fn saved_workspaces_survive_the_node_going_away() {
    let mut w = simple_world();
    w.connect(0);
    w.observe_saved(0, vec![saved("c-1", "nga", SavedHealthCode::Running)]);
    w.disconnect(0);
    assert_eq!(
        w.node_view("node-a").unwrap().saved.len(),
        1,
        "last-known, like panes"
    );
}

#[test]
fn an_orchestrator_that_did_not_accept_the_capability_gets_no_delta_and_no_sequence_gap() {
    let mut w = simple_world();
    // An orchestrator that knows nothing of saved workspaces accepts nothing of the sort.
    let offered: Vec<&str> = capability::KNOWN
        .iter()
        .copied()
        .filter(|c| *c != capability::SAVED_WORKSPACES)
        .collect();
    w.connect_offering(0, &offered);
    w.observe_saved(0, vec![saved("c-1", "nga", SavedHealthCode::Stopped)]);
    assert!(w.node_view("node-a").unwrap().saved.is_empty());
    // Pane deltas after it still apply in sequence (no gap was left behind).
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    assert_eq!(w.node_view("node-a").unwrap().panes.len(), 1);
    assert_eq!(w.resyncs, 0);
}

#[test]
fn an_invalid_report_is_rejected_as_a_violation() {
    use flight_proto::{delta_change, node_body, Delta, NodeFrame, SavedWorkspaces};
    let mut w = simple_world();
    let conn = w.connect(0);
    let mut bad = saved("not valid!", "nga", SavedHealthCode::Stopped);
    bad.config_key = "not valid!".to_owned();
    let incarnation = w.nodes[0].session.core().incarnation();
    let frame = NodeFrame {
        body: Some(node_body::Body::Delta(Delta {
            incarnation: incarnation.as_bytes().to_vec(),
            sequence: 1,
            change: Some(delta_change::Change::Saved(SavedWorkspaces {
                items: vec![bad],
            })),
        })),
    };
    let fx = w.orch.on_node_frame(conn, frame, w.now);
    assert_eq!(fx.close.len(), 1);
    assert!(w.node_view("node-a").unwrap().saved.is_empty());
}
