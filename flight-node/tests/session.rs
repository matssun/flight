// SPDX-License-Identifier: MIT

//! The node's side of the stream as a frame state machine: handshake, snapshot, deltas,
//! resync, and routed control requests, with no sockets.

mod support;

use flight_node::{
    Control, ControlError, NodeSession, SessionOutput, SessionRequest, ADVERTISED_CAPABILITIES,
};
use flight_proto::{
    command_kind as ck, node_body, orchestrator_body, response_result, Command, ErrorKindCode,
    Goodbye, Heartbeat, NodeFrame, OrchestratorFrame, OrchestratorHello, PaneRefMsg, ProgramCode,
    ProtocolVersion, Request, ResyncRequest, Validate, CURRENT_VERSION,
};
use flight_state::{PaneId, ServerId};
use std::sync::Mutex;
use support::*;

#[derive(Default)]
struct FakeControl {
    killed: Mutex<Vec<String>>,
    revealed: Mutex<Vec<String>>,
    created: Mutex<Vec<String>>,
}

impl Control for FakeControl {
    fn capture(&self, _: &ServerId, pane: &PaneId, lines: u32) -> Result<String, ControlError> {
        Ok(format!("{pane} last {lines} lines"))
    }
    fn kill_pane(&self, _: &ServerId, pane: &PaneId, pid: u32) -> Result<(), ControlError> {
        self.killed.lock().unwrap().push(format!("{pane}@{pid}"));
        Ok(())
    }
    fn reveal_pane(&self, _: &ServerId, pane: &PaneId, pid: u32) -> Result<(), ControlError> {
        self.revealed.lock().unwrap().push(format!("{pane}@{pid}"));
        Ok(())
    }
    fn create_session(&self, r: &SessionRequest) -> Result<(), ControlError> {
        self.created
            .lock()
            .unwrap()
            .push(format!("{}@{}:{:?}", r.name, r.dir, r.program));
        Ok(())
    }
}

thread_local! {
    /// One fake control per test thread: jobs are executed here, off the session, exactly as
    /// the transport does.
    static CONTROL: FakeControl = FakeControl::default();
}

fn killed() -> Vec<String> {
    CONTROL.with(|c| c.killed.lock().unwrap().clone())
}

fn created() -> Vec<String> {
    CONTROL.with(|c| c.created.lock().unwrap().clone())
}

fn revealed() -> Vec<String> {
    CONTROL.with(|c| c.revealed.lock().unwrap().clone())
}

fn session() -> NodeSession {
    NodeSession::new(core(), "mini-1")
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
fn ready(caps: &[&str]) -> NodeSession {
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

/// The single response a request produced, running its control job (if any) like the
/// transport does: after the session call has returned.
fn only_response(out: SessionOutput) -> response_result::Result {
    let mut frames = out.frames;
    for job in out.jobs {
        frames.push(CONTROL.with(|c| job.execute(c, 555)));
    }
    assert_eq!(frames.len(), 1);
    match body(&frames[0]) {
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
    assert!(killed().is_empty());
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
    assert!(killed().is_empty());

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
    assert_eq!(killed(), vec![format!("%1@{}", 7)]);
}

#[test]
fn input_is_unsupported_on_a_node() {
    let mut s = ready(&["preview", "kill", "create_session_v1", "send_input"]);
    let out = s.on_frame(
        request(
            6,
            ck::Kind::SendInput(ck::SendInput {
                pane_ref: pane("%1"),
                text: "y".into(),
                enter: true,
            }),
        ),
        1,
    );
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::Unsupported as i32
    );
}

fn reveal(id: u64, pane_id: &str, expected_pid: u32) -> OrchestratorFrame {
    request(
        id,
        ck::Kind::RevealPane(ck::RevealPane {
            pane_ref: pane(pane_id),
            expected_pid,
        }),
    )
}

#[test]
fn reveal_runs_when_the_caller_saw_the_published_process() {
    let mut s = ready(&["guarded_reveal_v1"]);
    let out = s.on_frame(reveal(8, "%1", 7), 1);
    assert!(matches!(
        only_response(out),
        response_result::Result::Done(_)
    ));
    assert_eq!(revealed(), vec!["%1@7".to_owned()]);
}

#[test]
fn reveal_of_a_replaced_pane_is_refused_before_anything_runs() {
    // The node publishes pid 7; the caller was looking at an earlier process under the same id.
    let mut s = ready(&["guarded_reveal_v1"]);
    let out = s.on_frame(reveal(9, "%1", 6), 1);
    assert!(out.jobs.is_empty(), "a stale reveal must not become a job");
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::PaneChanged as i32
    );
    assert!(revealed().is_empty());
}

#[test]
fn reveal_of_an_unknown_or_foreign_pane_is_refused() {
    let mut s = ready(&["guarded_reveal_v1"]);
    let unknown = s.on_frame(reveal(10, "%99", 7), 1);
    assert_eq!(
        error_kind(only_response(unknown)),
        ErrorKindCode::UnknownPane as i32
    );
    assert!(revealed().is_empty());
}

#[test]
fn a_peer_that_did_not_accept_guarded_reveal_gets_no_reveal_at_all() {
    // An orchestrator from before the guarded capability accepted only the older set. There
    // is no unguarded fallback: the request is refused, whatever pid it names.
    let mut s = ready(&["preview", "kill", "create_session_v1"]);
    let out = s.on_frame(reveal(11, "%1", 7), 1);
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::Unsupported as i32
    );
    assert!(revealed().is_empty());
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
    assert_eq!(out.jobs.len(), 1);
    assert_eq!(out.jobs[0].request_id(), 41);
    let frame = CONTROL.with(|c| out.jobs[0].clone().execute(c, 1));
    assert!(matches!(body(&frame), node_body::Body::Response(r) if r.request_id == 41));
}

#[test]
fn a_valid_request_only_plans_work_the_session_never_performs_it() {
    let mut s = ready(&["kill"]);
    let out = s.on_frame(
        request(
            8,
            ck::Kind::KillPane(ck::KillPane {
                pane_ref: pane("%1"),
            }),
        ),
        1,
    );
    assert!(out.frames.is_empty(), "no answer until the job has run");
    assert_eq!(out.jobs.len(), 1);
    assert!(
        killed().is_empty(),
        "nothing was killed inside the session call"
    );
}

fn open_terminal(id: u64, pane_id: &str, pid: u32) -> OrchestratorFrame {
    request(
        id,
        ck::Kind::OpenTerminal(ck::OpenTerminal {
            pane_ref: pane(pane_id),
            expected_pid: pid,
            cols: 90,
            rows: 25,
            term: "xterm".into(),
            terminal_id: vec![5; 16],
        }),
    )
}

#[test]
fn an_open_for_the_published_process_becomes_a_terminal_job_and_nothing_else() {
    let mut s = ready(&["terminal_v1"]);
    let out = s.on_frame(open_terminal(30, "%1", 7), 1);
    assert!(out.frames.is_empty());
    let [job] = out.jobs.as_slice() else {
        panic!("one job")
    };
    let spec = job.terminal().expect("a terminal job");
    assert_eq!((spec.pid, spec.cols, spec.rows), (7, 90, 25));
    assert_eq!(spec.terminal_id, [5; 16]);
    assert_eq!(spec.pane.as_str(), "%1");
    assert_eq!(spec.term, "xterm");
}

#[test]
fn a_stale_open_is_refused_before_a_job_exists() {
    let mut s = ready(&["terminal_v1"]);
    let out = s.on_frame(open_terminal(31, "%1", 8), 1);
    assert!(out.jobs.is_empty());
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::PaneChanged as i32
    );
}

#[test]
fn a_peer_that_did_not_accept_terminals_gets_none() {
    let mut s = ready(&["preview", "guarded_reveal_v1"]);
    let out = s.on_frame(open_terminal(32, "%1", 7), 1);
    assert!(out.jobs.is_empty());
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::Unsupported as i32
    );
}

#[test]
fn a_node_started_without_terminals_does_not_offer_them() {
    let mut s = NodeSession::new(core(), "mini-1").without_terminal();
    let hello = s.connect(vec![]);
    let node_body::Body::Hello(h) = body(&hello) else {
        panic!()
    };
    assert!(!h.capabilities.iter().any(|c| c == "terminal_v1"));
    assert!(h.capabilities.iter().any(|c| c == "guarded_reveal_v1"));
    // Even an orchestrator that accepts it (it was never offered) gets nothing.
    s.observe(vec![round(
        &server(),
        100,
        vec![obs("%1", 7, PERMIT_SCREEN, false)],
    )]);
    s.on_frame(orch_hello(&["terminal_v1"]), 100);
    let out = s.on_frame(open_terminal(33, "%1", 7), 1);
    assert!(out.jobs.is_empty());
}

#[test]
fn an_open_without_the_orchestrators_id_is_not_accepted() {
    let mut s = ready(&["terminal_v1"]);
    let mut frame = open_terminal(34, "%1", 7);
    if let Some(orchestrator_body::Body::Request(r)) = frame.body.as_mut() {
        if let Some(ck::Kind::OpenTerminal(o)) = r.command.as_mut().and_then(|c| c.kind.as_mut()) {
            o.terminal_id.clear();
        }
    }
    let out = s.on_frame(frame, 1);
    assert!(out.jobs.is_empty());
    assert!(out.close.is_some(), "an invalid frame ends the stream");
}

fn create(id: u64, host: &str, name: &str, program: ProgramCode) -> OrchestratorFrame {
    request(
        id,
        ck::Kind::CreateSession(ck::CreateSession {
            host: host.into(),
            name: name.into(),
            dir: "/work".into(),
            program: program as i32,
        }),
    )
}

#[test]
fn a_create_session_for_this_node_becomes_a_job_with_the_typed_program() {
    let host = "node-1";
    let mut s = ready(&["create_session_v1"]);
    for (name, program) in [("a", ProgramCode::Claude), ("b", ProgramCode::Shell)] {
        let out = s.on_frame(create(20, host, name, program), 1);
        assert_eq!(out.jobs.len(), 1);
        assert!(matches!(
            only_response(out),
            response_result::Result::Done(_)
        ));
    }
    assert_eq!(created(), ["a@/work:Claude", "b@/work:Shell"]);
}

#[test]
fn a_create_session_for_another_host_is_refused_and_creates_nothing() {
    let mut s = ready(&["create_session_v1"]);
    let out = s.on_frame(create(21, "someone-else", "a", ProgramCode::Shell), 1);
    assert!(out.jobs.is_empty());
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::InvalidRequest as i32
    );
    assert!(created().is_empty());
}

#[test]
fn a_peer_that_only_accepted_the_retired_create_capability_creates_nothing() {
    // An orchestrator from before `create_session_v1` negotiated the old name. Its request
    // would carry no program this node honours: refuse it, never run it as a shell.
    let host = "node-1";
    let mut s = ready(&["preview", "create_session"]);
    let out = s.on_frame(create(22, host, "a", ProgramCode::Claude), 1);
    assert_eq!(
        error_kind(only_response(out)),
        ErrorKindCode::Unsupported as i32
    );
    assert!(created().is_empty());
}

#[test]
fn a_malformed_create_session_closes_the_stream_before_any_job() {
    let host = "node-1";
    let mut s = ready(&["create_session_v1"]);
    for bad in ["a b", "a;rm", "", "../x"] {
        let out = s.on_frame(create(23, host, bad, ProgramCode::Shell), 1);
        assert!(out.jobs.is_empty(), "{bad:?}");
        assert!(out.close.is_some(), "{bad:?}");
    }
    assert!(created().is_empty());
}
