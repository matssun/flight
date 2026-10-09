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

mod actions {
    use super::*;
    use flight_proto::{
        command_kind as ck, ui_request_body, Command, ErrorKindCode, Request, SavedActionCode,
        UiRequest,
    };

    fn ask(host: &str, id: u64) -> Request {
        Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::SavedAction(ck::SavedAction {
                    host: host.to_owned(),
                    config_key: "c-1".to_owned(),
                    action: SavedActionCode::Retry as i32,
                    root: String::new(),
                })),
            }),
        }
    }

    fn run(w: &mut World, request: Request) {
        let fx = w.orch.ui_request(
            UiId(1),
            UiRequest {
                body: Some(ui_request_body::Body::Command(request)),
            },
            w.now,
        );
        w.deliver(fx);
    }

    fn error_kind(w: &World) -> Option<i32> {
        w.responses.iter().rev().find_map(|(_, e)| match &e.body {
            Some(flight_proto::ui_event_body::Body::Response(r)) => match &r.result {
                Some(flight_proto::response_result::Result::Error(e)) => Some(e.kind),
                _ => None,
            },
            _ => None,
        })
    }

    #[test]
    fn a_saved_action_is_routed_to_the_node_it_names() {
        let mut w = simple_world();
        w.subscribe(UiId(1));
        w.connect(0);
        w.connect(1);
        run(&mut w, ask("node-a", 7));
        assert_eq!(w.forwarded.len(), 1);
        assert_eq!(w.forwarded[0].0, "node-a");
    }

    #[test]
    fn a_node_that_does_not_offer_saved_actions_is_refused_at_once() {
        let mut w = simple_world();
        w.subscribe(UiId(1));
        let offered: Vec<&str> = capability::KNOWN
            .iter()
            .copied()
            .filter(|c| *c != capability::SAVED_ACTIONS)
            .collect();
        w.connect_offering(0, &offered);
        run(&mut w, ask("node-a", 8));
        assert!(w.forwarded.is_empty());
        assert_eq!(error_kind(&w), Some(ErrorKindCode::Unsupported as i32));
    }

    #[test]
    fn an_unknown_or_disconnected_node_is_an_error_not_a_queue() {
        let mut w = simple_world();
        w.subscribe(UiId(1));
        w.connect(0);
        run(&mut w, ask("nobody", 9));
        assert_eq!(error_kind(&w), Some(ErrorKindCode::InvalidRequest as i32));
        w.disconnect(0);
        run(&mut w, ask("node-a", 10));
        assert_eq!(error_kind(&w), Some(ErrorKindCode::NodeUnreachable as i32));
        assert!(w.forwarded.is_empty());
    }

    #[test]
    fn the_node_turns_it_into_a_job_only_for_itself_and_only_once_accepted() {
        let mut w = simple_world();
        w.connect(0);
        let frame = |host: &str| flight_proto::OrchestratorFrame {
            body: Some(flight_proto::orchestrator_body::Body::Request(ask(host, 1))),
        };
        let own = w.nodes[0].session.on_frame(frame("node-a"), w.now);
        assert_eq!(own.jobs.len(), 1);
        let other = w.nodes[0].session.on_frame(frame("node-b"), w.now);
        assert!(other.jobs.is_empty() && other.frames.len() == 1);
    }
}
