// SPDX-License-Identifier: MIT

//! Ported from Fleet's `deriveStatusFromEvents` (src/state/events.ts, MIT, (c) 2026 Nick
//! Nisi; see THIRD_PARTY.md).

use super::{EventEntry, EventKind, NotificationType};
use flight_state::AgentState;

const ASK_USER_QUESTION: &str = "AskUserQuestion";

/// State implied by the most recent event, or `None` when the log is empty or its last
/// event says nothing about state.
pub fn derive_status_from_events(events: &[EventEntry]) -> Option<AgentState> {
    let (last, earlier) = events.split_last()?;
    match last.kind {
        // The dashboard writes this when you view a ready agent: it leaves the attention
        // tier until the agent does something new.
        EventKind::Acknowledged => Some(AgentState::Idle),
        EventKind::Stop | EventKind::SubagentStop => Some(stop_state(last)),
        // AskUserQuestion is the agent asking *you*, not running a tool.
        EventKind::PreToolUse if is_ask_user_question(last) => Some(AgentState::Question),
        EventKind::PreToolUse => Some(AgentState::Busy),
        EventKind::Notification => notification_state(last, earlier),
        EventKind::Other => None,
    }
}

fn stop_state(stop: &EventEntry) -> AgentState {
    if stop.background_tasks || stop.stop_reason.as_deref() == Some("tool_use") {
        AgentState::Busy
    } else {
        AgentState::Done
    }
}

fn notification_state(last: &EventEntry, earlier: &[EventEntry]) -> Option<AgentState> {
    match last.notification_type? {
        // Indistinguishable from the notification alone: trace back to the tool that
        // triggered it.
        NotificationType::PermissionPrompt if triggered_by_ask_user_question(earlier) => {
            Some(AgentState::Question)
        }
        NotificationType::PermissionPrompt => Some(AgentState::Permit),
        NotificationType::ElicitationDialog => Some(AgentState::Question),
        NotificationType::IdlePrompt => Some(AgentState::Done),
        NotificationType::Other => None,
    }
}

fn is_ask_user_question(e: &EventEntry) -> bool {
    e.tool.as_deref() == Some(ASK_USER_QUESTION)
}

/// Walk back to the `PreToolUse` that opened the prompt. A `Stop`/`SubagentStop` in
/// between means a turn ended, so the prompt belongs to a later, unrelated request.
fn triggered_by_ask_user_question(earlier: &[EventEntry]) -> bool {
    for e in earlier.iter().rev() {
        match e.kind {
            EventKind::Stop | EventKind::SubagentStop => return false,
            EventKind::PreToolUse => return is_ask_user_question(e),
            _ => {}
        }
    }
    false
}
