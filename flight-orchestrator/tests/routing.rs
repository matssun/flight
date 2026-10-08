// SPDX-License-Identifier: MIT

//! Control requests: routed by stable id to a connected node only, never queued.

mod support;

use flight_orchestrator::UiId;
use flight_proto::{
    command_kind as ck, node_body, orchestrator_body, response_result, ui_event_body,
    ui_request_body, Command, ErrorKindCode, NodeFrame, ProgramCode, Request, Response, UiRequest,
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

fn reveal(id: u64, node: &str, pane: &str, expected_pid: u32) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::RevealPane(ck::RevealPane {
                    pane_ref: Some(pane_ref(node, pane)),
                    expected_pid,
                })),
            }),
        })),
    }
}

#[test]
fn a_reveal_is_forwarded_to_a_node_that_offers_the_guarded_capability() {
    let mut w = ready();
    send(&mut w, reveal(1, "node-a", "%1", 1));
    assert!(error_kinds(&w).is_empty(), "{:?}", error_kinds(&w));
    assert_eq!(w.forwarded.len(), 1);
}

#[test]
fn an_older_node_without_the_guarded_capability_is_never_sent_a_reveal() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect_offering(0, &["preview", "kill", "create_session_v1"]);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    send(&mut w, reveal(1, "node-a", "%1", 1));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::Unsupported as i32]);
    assert!(
        w.forwarded.is_empty(),
        "an unguarded older node must not see the request"
    );
}

#[test]
fn a_reveal_without_the_pane_process_never_reaches_routing() {
    let mut w = ready();
    send(&mut w, reveal(2, "node-a", "%1", 0));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::InvalidRequest as i32]);
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

fn forwarded_request(w: &World) -> Request {
    let Some(orchestrator_body::Body::Request(r)) =
        w.forwarded.last().expect("forwarded").1.body.clone()
    else {
        panic!("not a request")
    };
    r
}

fn node_error(request_id: u64, kind: ErrorKindCode) -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Response(Response {
            request_id,
            result: Some(response_result::Result::Error(flight_proto::ErrorInfo {
                kind: kind as i32,
                message: "remote".into(),
            })),
        })),
    }
}

#[test]
fn a_reveal_reaches_the_node_with_the_callers_pid_untouched() {
    let mut w = ready();
    // The pane the orchestrator knows has pid 1; the caller saw 100. The orchestrator
    // must not repair, compare or rewrite it: the node is the authority.
    send(&mut w, reveal(7, "node-a", "%1", 100));
    let sent = forwarded_request(&w);
    let Some(ck::Kind::RevealPane(r)) = sent.command.and_then(|c| c.kind) else {
        panic!("not a reveal")
    };
    assert_eq!(r.expected_pid, 100);
    assert_eq!(r.pane_ref, Some(pane_ref("node-a", "%1")));
}

#[test]
fn the_nodes_pane_changed_answer_reaches_the_ui_as_pane_changed() {
    let mut w = ready();
    send(&mut w, reveal(7, "node-a", "%1", 100));
    let conn = w.nodes[0].conn.expect("conn");
    let id = forwarded_request(&w).request_id;
    w.send_to_orch(conn, node_error(id, ErrorKindCode::PaneChanged));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::PaneChanged as i32]);
}

#[test]
fn a_reveal_in_flight_fails_on_disconnect_and_a_late_answer_is_ignored() {
    let mut w = ready();
    send(&mut w, reveal(7, "node-a", "%1", 1));
    let old = w.nodes[0].conn.expect("conn");
    let id = forwarded_request(&w).request_id;
    w.disconnect(0);
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
    // The node comes back; a response from the dead connection must not complete anything.
    w.connect(0);
    let before = w.responses.len();
    w.send_to_orch(
        old,
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id: id,
                result: Some(response_result::Result::Done(response_result::Done {})),
            })),
        },
    );
    assert_eq!(w.responses.len(), before);
}

#[test]
fn a_reveal_the_node_never_answers_times_out_with_a_typed_error() {
    let mut w = ready();
    send(&mut w, reveal(7, "node-a", "%1", 1));
    for _ in 0..3 {
        w.advance(5);
    }
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
}

fn create(id: u64, node: &str, name: &str, program: ProgramCode) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::CreateSession(ck::CreateSession {
                    host: node.into(),
                    server: "flight".into(),
                    name: name.into(),
                    dir: "/work".into(),
                    program: program as i32,
                })),
            }),
        })),
    }
}

fn two_nodes() -> World {
    let mut w = ready();
    w.connect(1);
    w.forwarded.clear();
    w
}

#[test]
fn a_create_session_reaches_only_the_node_it_names_and_the_answer_comes_back() {
    let mut w = two_nodes();
    send(&mut w, create(5, "node-b", "api", ProgramCode::Claude));
    assert!(error_kinds(&w).is_empty(), "{:?}", error_kinds(&w));
    assert_eq!(w.forwarded.len(), 1);
    let (node, frame) = w.forwarded[0].clone();
    assert_eq!(node, "node-b", "sent to node-b, not node-a");
    let conn = w.nodes[1].conn.expect("conn");
    let Some(orchestrator_body::Body::Request(forwarded)) = frame.body else {
        panic!("not a request")
    };
    // The typed request is forwarded as it was: host, name, directory, program.
    let Some(ck::Kind::CreateSession(c)) = forwarded.command.and_then(|c| c.kind) else {
        panic!("not a create")
    };
    assert_eq!(
        (c.host.as_str(), c.name.as_str(), c.dir.as_str()),
        ("node-b", "api", "/work")
    );
    assert_eq!(c.program, ProgramCode::Claude as i32);

    w.send_to_orch(
        conn,
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id: forwarded.request_id,
                result: Some(response_result::Result::Done(response_result::Done {})),
            })),
        },
    );
    let (ui, event) = w.responses.last().expect("response").clone();
    assert_eq!(ui, UiId(1));
    assert!(matches!(
        event.body,
        Some(ui_event_body::Body::Response(Response {
            request_id: 5,
            result: Some(response_result::Result::Done(_)),
        }))
    ));
}

#[test]
fn a_typed_refusal_from_the_node_reaches_the_ui_unchanged() {
    let mut w = two_nodes();
    send(&mut w, create(6, "node-a", "api", ProgramCode::Shell));
    let (_, frame) = w.forwarded[0].clone();
    let conn = w.nodes[0].conn.expect("conn");
    let Some(orchestrator_body::Body::Request(forwarded)) = frame.body else {
        panic!("not a request")
    };
    for kind in [
        ErrorKindCode::AlreadyExists,
        ErrorKindCode::InvalidDirectory,
        ErrorKindCode::ProgramUnavailable,
    ] {
        let mut w = two_nodes();
        send(&mut w, create(6, "node-a", "api", ProgramCode::Shell));
        w.send_to_orch(
            conn,
            NodeFrame {
                body: Some(node_body::Body::Response(Response {
                    request_id: forwarded.request_id,
                    result: Some(response_result::Result::Error(flight_proto::ErrorInfo {
                        kind: kind as i32,
                        message: "no".into(),
                    })),
                })),
            },
        );
        assert_eq!(error_kinds(&w), vec![kind as i32]);
    }
}

#[test]
fn a_create_session_for_a_disconnected_node_is_a_typed_unreachable_and_never_queued() {
    let mut w = two_nodes();
    w.disconnect(1);
    send(&mut w, create(7, "node-b", "api", ProgramCode::Shell));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
    assert!(w.forwarded.is_empty());
    w.connect(1);
    assert!(w.forwarded.is_empty(), "the old request is not delivered");
}

#[test]
fn a_node_without_the_versioned_capability_is_never_sent_a_create() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    // An older node: it offers the retired name, which carried a free-form command.
    w.connect_offering(0, &["preview", "kill", "create_session"]);
    send(&mut w, create(8, "node-a", "api", ProgramCode::Claude));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::Unsupported as i32]);
    assert!(w.forwarded.is_empty());
}

#[test]
fn a_malformed_or_oversized_create_never_reaches_a_node() {
    let mut w = two_nodes();
    let long = "a".repeat(65);
    for name in ["", "a b", "a;b", long.as_str()] {
        send(&mut w, create(9, "node-a", name, ProgramCode::Shell));
    }
    let mut bad_dir = create(10, "node-a", "ok", ProgramCode::Shell);
    if let Some(ui_request_body::Body::Command(Request {
        command: Some(Command {
            kind: Some(ck::Kind::CreateSession(c)),
        }),
        ..
    })) = bad_dir.body.as_mut()
    {
        c.dir = "relative".into();
    }
    send(&mut w, bad_dir);
    assert_eq!(
        error_kinds(&w),
        vec![ErrorKindCode::InvalidRequest as i32; 5]
    );
    assert!(w.forwarded.is_empty());
}
