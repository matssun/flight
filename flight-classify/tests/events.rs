// SPDX-License-Identifier: MIT

//! Event-log derivation; cases ported from Fleet's events.test.ts (MIT, (c) 2026 Nick Nisi).

use flight_classify::{derive_status_from_events, EventEntry, EventKind, NotificationType};
use flight_state::AgentState::{self, Busy, Done, Idle, Permit, Question};

fn ev(kind: EventKind, ts: u64) -> EventEntry {
    EventEntry::new(kind, ts)
}

fn tool(name: &str, ts: u64) -> EventEntry {
    EventEntry {
        tool: Some(name.into()),
        ..ev(EventKind::PreToolUse, ts)
    }
}

fn stop(reason: &str, ts: u64) -> EventEntry {
    EventEntry {
        stop_reason: Some(reason.into()),
        ..ev(EventKind::Stop, ts)
    }
}

fn notify(t: NotificationType, ts: u64) -> EventEntry {
    EventEntry {
        notification_type: Some(t),
        ..ev(EventKind::Notification, ts)
    }
}

fn derive(events: &[EventEntry]) -> Option<AgentState> {
    derive_status_from_events(events)
}

#[test]
fn empty_log_is_none() {
    assert_eq!(derive(&[]), None);
}

#[test]
fn stop_reasons() {
    assert_eq!(derive(&[stop("tool_use", 1)]), Some(Busy));
    assert_eq!(derive(&[stop("end_turn", 1)]), Some(Done));
    let bg = EventEntry {
        background_tasks: true,
        ..stop("end_turn", 1)
    };
    assert_eq!(derive(&[bg]), Some(Busy), "background tasks suppress Done");
}

#[test]
fn tool_use_is_busy_but_ask_user_question_is_a_question() {
    assert_eq!(derive(&[tool("Edit", 1)]), Some(Busy));
    assert_eq!(derive(&[tool("AskUserQuestion", 1)]), Some(Question));
}

#[test]
fn notification_types() {
    assert_eq!(
        derive(&[notify(NotificationType::PermissionPrompt, 1)]),
        Some(Permit)
    );
    assert_eq!(
        derive(&[notify(NotificationType::ElicitationDialog, 1)]),
        Some(Question)
    );
    assert_eq!(
        derive(&[notify(NotificationType::IdlePrompt, 1)]),
        Some(Done)
    );
    assert_eq!(derive(&[notify(NotificationType::Other, 1)]), None);
}

#[test]
fn permission_prompt_is_traced_to_the_triggering_tool() {
    let p = notify(NotificationType::PermissionPrompt, 2);
    assert_eq!(
        derive(&[tool("AskUserQuestion", 1), p.clone()]),
        Some(Question)
    );
    assert_eq!(derive(&[tool("Bash", 1), p.clone()]), Some(Permit));
    let p3 = notify(NotificationType::PermissionPrompt, 3);
    assert_eq!(
        derive(&[tool("AskUserQuestion", 1), stop("end_turn", 2), p3]),
        Some(Permit),
        "a Stop ends the earlier turn"
    );
    assert_eq!(derive(&[p]), Some(Permit), "no preceding tool");
}

#[test]
fn acknowledged_clears_a_ready_turn_until_new_activity() {
    assert_eq!(
        derive(&[stop("end_turn", 1), ev(EventKind::Acknowledged, 2)]),
        Some(Idle)
    );
    assert_eq!(
        derive(&[ev(EventKind::Acknowledged, 1), tool("Edit", 2)]),
        Some(Busy)
    );
}

#[test]
fn unknown_events_say_nothing() {
    assert_eq!(derive(&[ev(EventKind::Other, 1)]), None);
    assert_eq!(EventKind::from_wire("Whatever"), EventKind::Other);
    assert_eq!(
        NotificationType::from_wire("permission_prompt"),
        NotificationType::PermissionPrompt
    );
}
