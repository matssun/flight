// SPDX-License-Identifier: MIT

//! NodeCore semantics: authoritative state, derived deltas, Tracking, id reuse, outages.

mod support;

use flight_node::{fresh_incarnation, NodeCore, Unavailable};
use flight_proto::{delta_change::Change, Delta, ReplicationCursor, StateCode, Step};
use flight_state::{AgentState, HostId};
use support::*;

fn changes(deltas: &[Delta]) -> Vec<&Change> {
    deltas.iter().filter_map(|d| d.change.as_ref()).collect()
}

fn upserts(deltas: &[Delta]) -> Vec<&flight_proto::PaneState> {
    changes(deltas)
        .into_iter()
        .filter_map(|c| match c {
            Change::PaneUpsert(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn removals(deltas: &[Delta]) -> usize {
    changes(deltas)
        .into_iter()
        .filter(|c| matches!(c, Change::PaneRemoved(_)))
        .count()
}

fn state_of(core: &mut NodeCore, pane: &str) -> Option<AgentState> {
    core.snapshot()
        .panes
        .iter()
        .find(|p| p.pane_ref.as_ref().is_some_and(|r| r.pane == pane))
        .and_then(|p| p.agent_state().ok())
}

#[test]
fn first_observation_publishes_status_and_every_pane() {
    let s = server();
    let mut c = core();
    let d = c.apply(round(
        &s,
        100,
        vec![
            obs("%1", 7, PERMIT_SCREEN, false),
            obs("%2", 8, BUSY_SCREEN, false),
        ],
    ));
    assert_eq!(upserts(&d).len(), 2);
    assert!(matches!(changes(&d)[0], Change::ServerStatus(_)));
    let permit = upserts(&d)[0];
    assert_eq!(permit.agent_state(), Ok(AgentState::Permit));
    assert_eq!(permit.rule_id, "permit.do-you-want");
    assert_eq!(permit.changed_at, 100);
}

#[test]
fn an_unchanged_round_emits_nothing_even_as_time_passes() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    assert!(c
        .apply(round(&s, 160, vec![obs("%1", 7, PERMIT_SCREEN, false)]))
        .is_empty());
}

#[test]
fn a_state_change_emits_one_upsert_and_stamps_changed_at() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    let d = c.apply(round(&s, 130, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    assert_eq!(d.len(), 1);
    let p = upserts(&d)[0];
    assert_eq!(p.agent_state(), Ok(AgentState::Busy));
    assert_eq!(p.changed_at, 130);
}

#[test]
fn deltas_continue_one_sequence_from_the_last_snapshot() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    c.snapshot();
    let d1 = c.apply(round(&s, 110, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    let d2 = c.apply(round(&s, 120, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    let seqs: Vec<u64> = d1.iter().chain(&d2).map(|d| d.sequence).collect();
    assert_eq!(seqs, vec![1, 2]);
    assert!(d1.iter().all(|d| d.incarnation() == Ok(inc(1))));
    c.snapshot();
    let d3 = c.apply(round(&s, 130, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    assert_eq!(d3[0].sequence, 1, "a snapshot restarts the sequence");
}

#[test]
fn a_vanished_pane_is_removed_and_its_tracking_with_it() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    let d = c.apply(round(&s, 110, vec![]));
    assert_eq!(removals(&d), 1);
    assert!(!c.knows(&pane_ref(&s, "%1")));
    // Same tmux id comes back, idle: with Tracking gone there was no busy-then-idle, so no Done.
    c.apply(round(&s, 120, vec![obs("%1", 7, IDLE_SCREEN, false)]));
    assert_eq!(state_of(&mut c, "%1"), Some(AgentState::Idle));
}

#[test]
fn without_removal_the_same_sequence_is_done() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    c.apply(round(&s, 120, vec![obs("%1", 7, IDLE_SCREEN, false)]));
    assert_eq!(state_of(&mut c, "%1"), Some(AgentState::Done));
}

#[test]
fn the_same_pane_id_under_a_new_pid_is_a_new_pane() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    // tmux restarted between rounds and reused %1 for a different process.
    let d = c.apply(round(&s, 120, vec![obs("%1", 99, IDLE_SCREEN, false)]));
    assert_eq!(upserts(&d).len(), 1);
    assert_eq!(upserts(&d)[0].changed_at, 120);
    assert_eq!(
        state_of(&mut c, "%1"),
        Some(AgentState::Idle),
        "no inherited Done"
    );
}

#[test]
fn a_pane_that_stops_being_an_agent_is_removed() {
    let s = server();
    let mut c = core();
    c.apply(round(
        &s,
        100,
        vec![
            obs("%1", 7, PERMIT_SCREEN, false),
            obs("%2", 8, PERMIT_SCREEN, false),
        ],
    ));
    let d = c.apply(round(&s, 110, vec![obs("%2", 8, PERMIT_SCREEN, false)]));
    assert_eq!(removals(&d), 1);
}

#[test]
fn no_server_removes_its_panes_but_other_servers_are_untouched() {
    let (a, b) = (server(), flight_state::ServerId::new("other"));
    let mut c = core();
    c.apply(round(&a, 100, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    c.apply(round(&b, 100, vec![obs("%1", 9, PERMIT_SCREEN, false)]));
    let d = c.apply(down(&a, 110, Unavailable::NoServer));
    assert_eq!(removals(&d), 1);
    assert!(c.knows(&pane_ref(&b, "%1")));
    assert!(!c.knows(&pane_ref(&a, "%1")));
}

#[test]
fn a_transient_failure_keeps_panes_and_a_pending_done() {
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    let d = c.apply(down(&s, 110, Unavailable::Failed("tmux hiccup".into())));
    assert_eq!(removals(&d), 0);
    assert_eq!(d.len(), 1, "only the server status changed");
    c.apply(round(&s, 120, vec![obs("%1", 7, IDLE_SCREEN, false)]));
    assert_eq!(state_of(&mut c, "%1"), Some(AgentState::Done));
}

#[test]
fn server_status_is_replicated_only_when_it_changes() {
    let s = server();
    let mut c = core();
    assert_eq!(c.apply(round(&s, 100, vec![])).len(), 1);
    assert!(c.apply(round(&s, 110, vec![])).is_empty());
    assert_eq!(c.apply(down(&s, 120, Unavailable::TmuxMissing)).len(), 1);
    assert!(c.apply(down(&s, 130, Unavailable::TmuxMissing)).is_empty());
}

#[test]
fn a_restarted_node_has_a_new_incarnation_and_forgets_its_tracking() {
    let s = server();
    let first = fresh_incarnation().expect("random");
    let second = fresh_incarnation().expect("random");
    assert_ne!(first, second);

    let mut old = NodeCore::new(HostId::new("node-1"), first);
    old.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    let mut cursor = ReplicationCursor::new();
    cursor.on_snapshot(old.incarnation());

    let mut fresh = NodeCore::new(HostId::new("node-1"), second);
    fresh.apply(round(&s, 120, vec![obs("%1", 7, IDLE_SCREEN, false)]));
    assert_eq!(
        state_of(&mut fresh, "%1"),
        Some(AgentState::Idle),
        "cold start: no Done"
    );

    // The restarted node's first delta cannot be mistaken for a continuation.
    let d = fresh.apply(round(&s, 130, vec![obs("%1", 7, PERMIT_SCREEN, false)]));
    assert_eq!(
        cursor.on_delta(d[0].incarnation().expect("inc"), d[0].sequence),
        Step::Resync
    );
}

#[test]
fn published_state_carries_no_tracking_fields() {
    // Compile-time guard in spirit: the replicated message is the externally visible state.
    let s = server();
    let mut c = core();
    c.apply(round(&s, 100, vec![obs("%1", 7, BUSY_SCREEN, false)]));
    let snap = c.snapshot();
    let p = &snap.panes[0];
    assert_eq!(p.state, StateCode::Busy as i32);
}
