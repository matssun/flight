// SPDX-License-Identifier: MIT

//! The node's side of the stream as a frame state machine: handshake, snapshot, deltas,
//! resync, and routed control requests, with no sockets.

mod support;

use flight_node::{Control, ControlError, NodeSession, SessionOutput, ADVERTISED_CAPABILITIES};
use flight_proto::{
    command_kind as ck, node_body, orchestrator_body, response_result, Command, ErrorKindCode,
    Goodbye, Heartbeat, NodeFrame, OrchestratorFrame, OrchestratorHello, PaneRefMsg,
    ProtocolVersion, Request, ResyncRequest, Validate, CURRENT_VERSION,
};
use flight_state::{PaneId, ServerId};
use std::sync::Mutex;
use support::*;

#[derive(Default)]
struct FakeControl {
    killed: Mutex<Vec<String>>,
}

impl Control for FakeControl {
    fn capture(&self, _: &ServerId, pane: &PaneId, lines: u32) -> Result<String, ControlError> {
        Ok(format!("{pane} last {lines} lines"))
    }
    fn kill_pane(&self, _: &ServerId, pane: &PaneId) -> Result<(), ControlError> {
        self.killed.lock().unwrap().push(pane.to_string());
        Ok(())
    }
    fn create_session(&self, _: &ServerId, _: &str, _: &str, _: &str) -> Result<(), ControlError> {
        Ok(())
    }
}

fn session() -> NodeSession<FakeControl> {
    NodeSession::new(core(), FakeControl::default(), "mini-1")
}

fn orch(body: orchestrator_body::Body) -> OrchestratorFrame {
    OrchestratorFrame { body: Some(body) }
}

fn orch_hello(caps: &[&str]) -> OrchestratorFrame {
    orch(orchestrator_body::Body::Hello(OrchestratorHello {
        version: Some(CURRENT_VERSION),
        accepted_capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
        heartbeat_interval_secs: 5,
    }))
}

fn request(id: u64, kind: ck::Kind) -> OrchestratorFrame {
    orch(orchestrator_body::Body::Request(Request {
        request_id: id,
        command: Some(Command { kind: Some(kind) }),
    }))
}

fn pane(pane: &str) -> Option<PaneRefMsg> {
    Some(PaneRefMsg::from(&pane_ref(&server(), pane)))
}

fn body(f: &NodeFrame) -> &node_body::Body {
    f.body.as_ref().expect("body")
}

/// A session with one permit pane, past the handshake.
fn ready(caps: &[&str]) -> NodeSession<FakeControl> {
    let mut s = session();
    s.observe(vec![round(
        &server(),
        100,
        vec![obs("%1", 7, PERMIT_SCREEN, false)],
    )]);
    s.connect(vec!["flight".into()]);
    s.on_frame(orch_hello(caps), 100);
    s
}

fn only_response(out: SessionOutput) -> response_result::Result {
    assert_eq!(out.frames.len(), 1);
    match body(&out.frames[0]) {
        node_body::Body::Response(r) => r.result.clone().expect("result"),
        other => panic!("not a response: {other:?}"),
    }
}

fn error_kind(r: response_result::Result) -> i32 {
    match r {
        response_result::Result::Error(e) => e.kind,
        other => panic!("not an error: {other:?}"),
    }
}

#[test]
fn connect_sends_a_valid_hello_advertising_only_what_the_node_can_do() {
    let mut s = session();
    let hello = s.connect(vec!["flight".into()]);
    assert_eq!(hello.validate(), Ok(()));
    let node_body::Body::Hello(h) = body(&hello) else {
        panic!("not a hello")
    };
    assert_eq!(h.node_id, "node-1");
    assert_eq!(h.display_name, "mini-1");
    assert_eq!(h.capabilities, ADVERTISED_CAPABILITIES);
    assert!(!h.capabilities.iter().any(|c| c == "send_input"));
}

#[test]
fn the_snapshot_follows_the_orchestrator_hello_and_contains_pre_handshake_state() {
    let mut s = session();
    // State observed before the handshake produces no frames...
    let early = s.observe(vec![round(
        &server(),
        100,
        vec![obs("%1", 7, PERMIT_SCREEN, false)],
    )]);
    assert!(early.is_empty());
    s.connect(vec![]);
    // ...because the snapshot sent after the hello already contains it.
    let out = s.on_frame(orch_hello(&["preview"]), 100);
    let node_body::Body::Snapshot(snap) = body(&out.frames[0]) else {
        panic!("expected snapshot")
    };
    assert_eq!(snap.panes.len(), 1);
    assert_eq!(out.frames[0].validate(), Ok(()));
}

#[test]
fn after_the_handshake_changes_flow_as_validated_deltas() {
    let mut s = ready(&["preview"]);
    let frames = s.observe(vec![round(
        &server(),
        110,
        vec![obs("%1", 7, BUSY_SCREEN, false)],
    )]);
    assert_eq!(frames.len(), 1);
    assert!(matches!(body(&frames[0]), node_body::Body::Delta(d) if d.sequence == 1));
    assert!(frames.iter().all(|f| f.validate().is_ok()));
}

#[test]
fn a_resync_request_yields_a_fresh_snapshot_and_restarts_the_sequence() {
    let mut s = ready(&["preview"]);
    s.observe(vec![round(
        &server(),
        110,
        vec![obs("%1", 7, BUSY_SCREEN, false)],
    )]);
    let out = s.on_frame(
        orch(orchestrator_body::Body::Resync(ResyncRequest {
            reason: "gap".into(),
        })),
        111,
    );
    assert!(matches!(body(&out.frames[0]), node_body::Body::Snapshot(_)));
    let next = s.observe(vec![round(
        &server(),
        120,
        vec![obs("%1", 7, PERMIT_SCREEN, false)],
    )]);
    assert!(matches!(body(&next[0]), node_body::Body::Delta(d) if d.sequence == 1));
}

#[test]
fn a_major_version_mismatch_closes_without_a_snapshot() {
    let mut s = session();
    s.connect(vec![]);
    let out = s.on_frame(
        orch(orchestrator_body::Body::Hello(OrchestratorHello {
            version: Some(ProtocolVersion {
                major: CURRENT_VERSION.major + 1,
                minor: 0,
            }),
            accepted_capabilities: vec![],
            heartbeat_interval_secs: 5,
        })),
        1,
    );
    assert!(out.frames.is_empty());
    assert!(out.close.is_some_and(|c| c.contains("major")));
}

#[test]
fn heartbeats_are_echoed_goodbye_closes_and_garbage_closes() {
    let mut s = ready(&["preview"]);
    let out = s.on_frame(
        orch(orchestrator_body::Body::Heartbeat(Heartbeat { seq: 9 })),
        1,
    );
    assert!(matches!(body(&out.frames[0]), node_body::Body::Heartbeat(h) if h.seq == 9));

    let bye = s.on_frame(
        orch(orchestrator_body::Body::Goodbye(Goodbye {
            reason: ErrorKindCode::NotAuthorized as i32,
            message: "revoked".into(),
        })),
        1,
    );
    assert!(bye.close.is_some());

    let bad = s.on_frame(OrchestratorFrame { body: None }, 1);
    assert!(bad.close.is_some());
}

#[test]
fn preview_returns_text_for_a_published_pane() {
    let mut s = ready(&["preview"]);
    let out = s.on_frame(
        request(
            1,
            ck::Kind::GetPreview(ck::GetPreview {
                pane_ref: pane("%1"),
                lines: 30,
            }),
        ),
        555,
    );
    match only_response(out) {
        response_result::Result::Preview(p) => {
            assert_eq!(p.text, "%1 last 30 lines");
            assert_eq!(p.captured_at, 555);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn requests_for_unknown_or_foreign_panes_are_refused_before_tmux_is_touched() {
    let mut s = ready(&["preview", "kill"]);
    let unknown = s.on_frame(
        request(
            2,
            ck::Kind::KillPane(ck::KillPane {
                pane_ref: pane("%99"),
            }),
        ),
        1,
    );
    assert_eq!(
        error_kind(only_response(unknown)),
        ErrorKindCode::UnknownPane as i32
    );
    let foreign = s.on_frame(
        request(
            3,
            ck::Kind::KillPane(ck::KillPane {
                pane_ref: Some(PaneRefMsg {
                    host: "other".into(),
                    server: "flight".into(),
                    pane: "%1".into(),
                }),
            }),
        ),
        1,
    );
    assert_eq!(
        error_kind(only_response(foreign)),
        ErrorKindCode::InvalidRequest as i32
    );
    assert!(s.control().killed.lock().unwrap().clone().is_empty());
}

#[test]
fn kill_runs_only_when_the_capability_was_accepted() {
    let mut denied = ready(&["preview"]);
    let out = denied.on_frame(
        request(
            4,
            ck::Kind::KillPane(ck::KillPane {
                pane_ref: pane("%1"),
            }),
        ),
        1,
    );
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::Unsupported as i32
    );
    assert!(denied.control().killed.lock().unwrap().clone().is_empty());

    let mut allowed = ready(&["kill"]);
    let out = allowed.on_frame(
        request(
            5,
            ck::Kind::KillPane(ck::KillPane {
                pane_ref: pane("%1"),
            }),
        ),
        1,
    );
    assert!(matches!(
        only_response(out),
        response_result::Result::Done(_)
    ));
    assert_eq!(allowed.control().killed.lock().unwrap().clone(), vec!["%1"]);
}

#[test]
fn switch_and_input_are_unsupported_on_a_node() {
    let mut s = ready(&["preview", "kill", "create_session", "switch", "send_input"]);
    for kind in [
        ck::Kind::SwitchPane(ck::SwitchPane {
            pane_ref: pane("%1"),
        }),
        ck::Kind::SendInput(ck::SendInput {
            pane_ref: pane("%1"),
            text: "y".into(),
            enter: true,
        }),
    ] {
        let out = s.on_frame(request(6, kind), 1);
        assert_eq!(
            error_kind(only_response(out)),
            ErrorKindCode::Unsupported as i32
        );
    }
}

#[test]
fn requests_before_the_handshake_are_not_authorized() {
    let mut s = session();
    s.connect(vec![]);
    let out = s.on_frame(
        request(
            7,
            ck::Kind::GetPreview(ck::GetPreview {
                pane_ref: pane("%1"),
                lines: 5,
            }),
        ),
        1,
    );
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::NotAuthorized as i32
    );
}

#[test]
fn responses_carry_the_request_id() {
    let mut s = ready(&["preview"]);
    let out = s.on_frame(
        request(
            41,
            ck::Kind::GetPreview(ck::GetPreview {
                pane_ref: pane("%1"),
                lines: 5,
            }),
        ),
        1,
    );
    assert!(matches!(body(&out.frames[0]), node_body::Body::Response(r) if r.request_id == 41));
}
