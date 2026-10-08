// SPDX-License-Identifier: MIT

//! Terminal admission and correlation in the orchestrator core: ids are minted here, bound to
//! the asking identity and the node connection, single-use per side, bounded, and dead after
//! any ending.

mod support;

use flight_orchestrator::{Side, TerminalId, UiId};
use flight_proto::{
    command_kind as ck, node_body, orchestrator_body, response_result, ui_event_body,
    ui_request_body, Command, ErrorKindCode, ExitReasonCode, NodeFrame, Request, Response,
    UiRequest,
};
use support::*;

const TERMINAL: &[&str] = &["preview", "guarded_reveal_v1", "terminal_v1"];

fn open(id: u64, node: &str, pane: &str, pid: u32) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::OpenTerminal(ck::OpenTerminal {
                    pane_ref: Some(pane_ref(node, pane)),
                    expected_pid: pid,
                    cols: 80,
                    rows: 24,
                    term: "xterm".into(),
                    terminal_id: Vec::new(),
                })),
            }),
        })),
    }
}

fn world() -> World {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect_offering(0, TERMINAL);
    w.observe(
        0,
        round(
            1,
            vec![
                obs("%1", 1, PERMIT_SCREEN),
                obs("%2", 2, PERMIT_SCREEN),
                obs("%3", 3, PERMIT_SCREEN),
                obs("%4", 4, PERMIT_SCREEN),
                obs("%5", 5, PERMIT_SCREEN),
            ],
        ),
    );
    w.orch.ui_identified(UiId(1), "ui-a");
    w.orch.ui_identified(UiId(2), "ui-b");
    w
}

fn send(w: &mut World, ui: UiId, req: UiRequest) {
    let fx = w.orch.ui_request(ui, req, w.now);
    w.deliver(fx);
}

fn errors(w: &World) -> Vec<i32> {
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

/// What the node was asked: its request id and the terminal id written into the command.
fn node_saw(w: &World) -> (u64, Vec<u8>) {
    let Some(orchestrator_body::Body::Request(r)) = w.forwarded.last().expect("fwd").1.body.clone()
    else {
        panic!()
    };
    let Some(ck::Kind::OpenTerminal(o)) = r.command.and_then(|c| c.kind) else {
        panic!()
    };
    (r.request_id, o.terminal_id)
}

fn node_answers(w: &mut World, request_id: u64, result: response_result::Result) {
    let conn = w.nodes[0].conn.expect("conn");
    w.send_to_orch(
        conn,
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id,
                result: Some(result),
            })),
        },
    );
}

fn done() -> response_result::Result {
    response_result::Result::Done(response_result::Done {})
}

fn opened_id(w: &World) -> Option<Vec<u8>> {
    w.responses.iter().rev().find_map(|(_, e)| match &e.body {
        Some(ui_event_body::Body::Response(Response {
            result: Some(response_result::Result::Terminal(t)),
            ..
        })) => Some(t.terminal_id.clone()),
        _ => None,
    })
}

/// Open and let the node answer; returns the id.
fn open_one(w: &mut World, ui: UiId, id: u64, pane: &str, pid: u32) -> Vec<u8> {
    send(w, ui, open(id, "node-a", pane, pid));
    let (req, term) = node_saw(w);
    node_answers(w, req, done());
    assert_eq!(opened_id(w), Some(term.clone()));
    term
}

#[test]
fn the_orchestrator_mints_the_id_and_the_node_is_told_it() {
    let mut w = world();
    send(&mut w, UiId(1), open(1, "node-a", "%1", 1));
    assert!(errors(&w).is_empty(), "{:?}", errors(&w));
    let (_, id) = node_saw(&w);
    assert_eq!(id, vec![1; 16], "the id is the orchestrator's");
    assert_eq!(w.orch.terminal_count(), 1);
}

#[test]
fn a_ui_that_names_its_own_id_is_refused_and_nothing_is_forwarded() {
    let mut w = world();
    let mut req = open(1, "node-a", "%1", 1);
    if let Some(ui_request_body::Body::Command(r)) = req.body.as_mut() {
        if let Some(ck::Kind::OpenTerminal(o)) = r.command.as_mut().and_then(|c| c.kind.as_mut()) {
            o.terminal_id = vec![9; 16];
        }
    }
    send(&mut w, UiId(1), req);
    assert_eq!(errors(&w), vec![ErrorKindCode::InvalidRequest as i32]);
    assert!(w.forwarded.is_empty());
    assert_eq!(w.orch.terminal_count(), 0);
}

#[test]
fn a_node_without_the_capability_is_never_sent_an_open() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect_offering(0, &["preview", "guarded_reveal_v1"]);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    send(&mut w, UiId(1), open(1, "node-a", "%1", 1));
    assert_eq!(errors(&w), vec![ErrorKindCode::Unsupported as i32]);
    assert!(w.forwarded.is_empty());
    assert_eq!(w.orch.terminal_count(), 0);
}

#[test]
fn the_node_answer_becomes_the_id_for_the_ui_and_an_error_kills_the_id() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    assert_eq!(id.len(), 16);

    send(&mut w, UiId(1), open(2, "node-a", "%2", 2));
    let (req, term) = node_saw(&w);
    node_answers(
        &mut w,
        req,
        response_result::Result::Error(flight_proto::ErrorInfo {
            kind: ErrorKindCode::PaneChanged as i32,
            message: "x".into(),
        }),
    );
    assert_eq!(
        errors(&w).last(),
        Some(&(ErrorKindCode::PaneChanged as i32))
    );
    let dead: TerminalId = term.try_into().unwrap();
    assert!(!w.orch.terminal_is_open(&dead));
    assert!(w.orch.terminal_attach(Side::Node, &dead, "node-a").is_err());
}

#[test]
fn each_side_attaches_once_as_the_identity_it_was_bound_to() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    // Strangers, other roles' identities and bad ids look identical: refused.
    assert!(w.orch.terminal_attach(Side::Ui, &id, "ui-b").is_err());
    assert!(w.orch.terminal_attach(Side::Ui, &id, "node-a").is_err());
    assert!(w.orch.terminal_attach(Side::Node, &id, "ui-a").is_err());
    assert!(w.orch.terminal_attach(Side::Node, &id, "node-b").is_err());
    assert!(w
        .orch
        .terminal_attach(Side::Node, &id[..15], "node-a")
        .is_err());
    assert!(w
        .orch
        .terminal_attach(Side::Node, &[7; 16], "node-a")
        .is_err());

    let (_, first) = w.orch.terminal_attach(Side::Ui, &id, "ui-a").unwrap();
    assert!(!first.both);
    assert!(
        w.orch.terminal_attach(Side::Ui, &id, "ui-a").is_err(),
        "single use"
    );
    let (_, second) = w.orch.terminal_attach(Side::Node, &id, "node-a").unwrap();
    assert!(second.both);
    assert!(
        w.orch.terminal_attach(Side::Node, &id, "node-a").is_err(),
        "single use"
    );
}

#[test]
fn the_ui_cannot_attach_before_the_node_has_answered() {
    let mut w = world();
    send(&mut w, UiId(1), open(1, "node-a", "%1", 1));
    let (req, id) = node_saw(&w);
    assert!(w.orch.terminal_attach(Side::Ui, &id, "ui-a").is_err());
    // The node, though, may dial before its answer is processed on the other connection.
    assert!(w.orch.terminal_attach(Side::Node, &id, "node-a").is_ok());
    node_answers(&mut w, req, done());
    assert!(w.orch.terminal_attach(Side::Ui, &id, "ui-a").is_ok());
}

#[test]
fn limits_refuse_at_once_with_busy_and_create_nothing() {
    let mut w = world();
    open_one(&mut w, UiId(1), 1, "%1", 1);
    open_one(&mut w, UiId(1), 2, "%2", 2);
    // A third for the same UI identity.
    let before = w.forwarded.len();
    send(&mut w, UiId(1), open(3, "node-a", "%3", 3));
    assert_eq!(errors(&w).last(), Some(&(ErrorKindCode::Busy as i32)));
    assert_eq!(w.forwarded.len(), before);
    assert_eq!(w.orch.terminal_count(), 2);
    // Another UI is fine until the node's own limit.
    open_one(&mut w, UiId(2), 4, "%3", 3);
    open_one(&mut w, UiId(2), 5, "%4", 4);
    assert_eq!(w.orch.terminal_count(), 4);
    send(&mut w, UiId(2), open(6, "node-a", "%5", 5));
    assert_eq!(errors(&w).last(), Some(&(ErrorKindCode::Busy as i32)));
}

#[test]
fn opening_the_same_pane_again_replaces_the_first_terminal() {
    let mut w = world();
    let first = open_one(&mut w, UiId(1), 1, "%1", 1);
    let second = open_one(&mut w, UiId(1), 2, "%1", 1);
    assert_ne!(first, second);
    let dead: TerminalId = first.try_into().unwrap();
    assert!(!w.orch.terminal_is_open(&dead));
    assert!(w.ended.contains(&(dead, ExitReasonCode::ClosedByUi)));
    assert_eq!(w.orch.terminal_count(), 1);
}

#[test]
fn an_open_nobody_attaches_to_expires_and_the_id_stays_dead() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    w.advance(11);
    let dead: TerminalId = id.try_into().unwrap();
    assert!(w.ended.iter().any(|(i, _)| *i == dead));
    assert!(w.orch.terminal_attach(Side::Ui, &dead, "ui-a").is_err());
    assert!(w.orch.terminal_attach(Side::Node, &dead, "node-a").is_err());
    assert_eq!(w.orch.terminal_count(), 0);
}

#[test]
fn a_live_terminal_does_not_expire() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    w.orch.terminal_attach(Side::Ui, &id, "ui-a").unwrap();
    w.orch.terminal_attach(Side::Node, &id, "node-a").unwrap();
    w.advance(5);
    w.advance(5);
    w.advance(5);
    let id: TerminalId = id.try_into().unwrap();
    assert!(w.orch.terminal_is_open(&id));
}

#[test]
fn a_node_that_goes_away_takes_its_terminals_with_it() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    w.orch.terminal_attach(Side::Ui, &id, "ui-a").unwrap();
    w.orch.terminal_attach(Side::Node, &id, "node-a").unwrap();
    w.disconnect(0);
    let dead: TerminalId = id.try_into().unwrap();
    assert!(w.ended.contains(&(dead, ExitReasonCode::NodeLost)));
    assert!(!w.orch.terminal_is_open(&dead));
}

#[test]
fn a_revoked_ui_loses_its_terminals_and_only_its_own() {
    let mut w = world();
    let mine = open_one(&mut w, UiId(1), 1, "%1", 1);
    let theirs = open_one(&mut w, UiId(2), 2, "%2", 2);
    let fx = w.orch.end_ui_terminals("ui-a", ExitReasonCode::Revoked);
    w.deliver(fx);
    let (mine, theirs): (TerminalId, TerminalId) =
        (mine.try_into().unwrap(), theirs.try_into().unwrap());
    assert!(w.ended.contains(&(mine, ExitReasonCode::Revoked)));
    assert!(w.orch.terminal_is_open(&theirs));
}

#[test]
fn an_ended_terminal_is_dead_for_good() {
    let mut w = world();
    let id = open_one(&mut w, UiId(1), 1, "%1", 1);
    w.orch.terminal_attach(Side::Ui, &id, "ui-a").unwrap();
    let dead: TerminalId = id.clone().try_into().unwrap();
    w.orch.terminal_ended(&dead);
    assert!(w.orch.terminal_attach(Side::Node, &id, "node-a").is_err());
    assert!(w.orch.terminal_attach(Side::Ui, &id, "ui-a").is_err());
}
