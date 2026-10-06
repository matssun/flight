// SPDX-License-Identifier: MIT

use crate::{AgentKind, Classification};
use flight_state::AgentState;

/// Codex retitles its pane "Action Required" for approvals and for native questions alike.
/// When the screen positively shows a `question.*` rule, that is more specific than the
/// shared title, so the screen result replaces it. Everything else keeps the title result.
pub fn refine_title_with_screen(
    agent: AgentKind,
    title: Classification,
    screen: Option<Classification>,
) -> Classification {
    match screen {
        Some(s)
            if agent == AgentKind::Codex
                && title.rule_id.as_str() == "permit.title-action-required"
                && s.state == AgentState::Question
                && s.rule_id.as_str().starts_with("question.") =>
        {
            s
        }
        _ => title,
    }
}
