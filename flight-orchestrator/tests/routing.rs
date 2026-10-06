// SPDX-License-Identifier: MIT

//! Control requests: routed by stable id to a connected node only, never queued.

mod support;

use flight_orchestrator::UiId;
use flight_proto::{
    command_kind as ck, node_body, orchestrator_body, response_result, ui_event_body,
    ui_request_body, Command, ErrorKindCode, NodeFrame, Request, Response, UiRequest,
};
use support::*;

fn kill(id: u64, node: &str, pane: &str) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::KillPane(ck::KillPane {
                    pane_ref: Some(pane_ref(node, pane)),
                })),
            }),
        })),
    }
}

fn error_kinds(w: &World) -> Vec<i32> {
    w.responses
        .iter()
        .filter_map(|(_, e)| match &e.body {
            Some(ui_event_body::Body::Response(Response {
                result: Some(response_result::Result::Error(e)),
                ..
            })) => Some(e.kind),
            _ => None,
        })
        .collect()
}

fn ready() -> World {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w
}

fn send(w: &mut World, req: UiRequest) {
    let fx = w.orch.ui_request(UiId(1), req, w.now);
    w.deliver(fx);
}

#[test]
fn a_command_goes_to_the_connected_node_under_a_fresh_id_and_the_answer_comes_back() {
    let mut w = ready();
    send(&mut w, kill(77, "node-a", "%1"));
    assert_eq!(w.forwarded.len(), 1);
    let Some(orchestrator_body::Body::Request(forwarded)) = w.forwarded[0].1.body.clone() else {
        panic!("not a request")
    };
    // The node-facing id is the orchestrator's own, not the UI's.
    let conn = w.nodes[0].conn.expect("conn");
    let reply = NodeFrame {
        body: Some(node_body::Body::Response(Response {
            request_id: forwarded.request_id,
            result: Some(response_result::Result::Done(response_result::Done {})),
        })),
    };
    w.send_to_orch(conn, reply);
    let (ui, event) = w.responses.last().expect("response").clone();
    assert_eq!(ui, UiId(1));
    assert!(matches!(
        event.body,
        Some(ui_event_body::Body::Response(Response {
            request_id: 77,
            ..
        }))
    ));
}

#[test]
fn a_command_for_a_disconnected_node_fails_at_once_and_is_never_queued() {
    let mut w = ready();
    w.disconnect(0);
    send(&mut w, kill(1, "node-a", "%1"));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
    assert!(w.forwarded.is_empty());
    // Reconnecting later does not deliver the old command.
    w.connect(0);
    assert!(w.forwarded.is_empty());
}

#[test]
fn a_stale_node_is_not_sent_commands() {
    let mut w = ready();
    w.mute(0);
    w.advance(16);
    send(&mut w, kill(1, "node-a", "%1"));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
    assert!(w.forwarded.is_empty());
}

#[test]
fn unknown_nodes_and_unoffered_capabilities_are_refused() {
    let mut w = ready();
    send(&mut w, kill(1, "node-zzz", "%1"));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::InvalidRequest as i32]);
    // send_input is not something the node advertises.
    let input = UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: 2,
            command: Some(Command {
                kind: Some(ck::Kind::SendInput(ck::SendInput {
                    pane_ref: Some(pane_ref("node-a", "%1")),
                    text: "y".into(),
                    enter: true,
                })),
            }),
        })),
    };
    send(&mut w, input);
    assert_eq!(error_kinds(&w)[1], ErrorKindCode::Unsupported as i32);
    assert!(w.forwarded.is_empty());
}

#[test]
fn an_invalid_command_is_rejected_before_routing() {
    let mut w = ready();
    send(
        &mut w,
        UiRequest {
            body: Some(ui_request_body::Body::Command(Request {
                request_id: 5,
                command: None,
            })),
        },
    );
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::InvalidRequest as i32]);
}

#[test]
fn in_flight_requests_fail_when_the_node_goes_away_and_when_they_time_out() {
    let mut w = ready();
    send(&mut w, kill(1, "node-a", "%1"));
    w.disconnect(0);
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);

    let mut w = ready();
    send(&mut w, kill(2, "node-a", "%1"));
    // Keep the node alive but silent about the request.
    for _ in 0..3 {
        w.advance(5);
    }
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
}

#[test]
fn a_response_from_the_wrong_connection_is_ignored() {
    let mut w = ready();
    w.connect(1);
    send(&mut w, kill(9, "node-a", "%1"));
    let Some(orchestrator_body::Body::Request(forwarded)) = w.forwarded[0].1.body.clone() else {
        panic!()
    };
    let other = w.nodes[1].conn.expect("b");
    w.send_to_orch(
        other,
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id: forwarded.request_id,
                result: Some(response_result::Result::Done(response_result::Done {})),
            })),
        },
    );
    assert!(
        w.responses.is_empty(),
        "node B cannot answer node A's request"
    );
}

#[test]
fn routing_follows_the_node_id_not_the_display_name() {
    let mut w = World::new(vec![
        SimNode::new("node-a", "studio", 1),
        SimNode::new("node-b", "studio", 1),
    ]);
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(1, round(1, vec![obs("%1", 2, PERMIT_SCREEN)]));
    send(&mut w, kill(1, "node-b", "%1"));
    assert_eq!(w.forwarded.len(), 1);
    assert_eq!(w.forwarded[0].0, "node-b");
}
