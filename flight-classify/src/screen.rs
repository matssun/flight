// SPDX-License-Identifier: MIT

use crate::ansi::strip_ansi;
use crate::{Classification, Manifest, Observation, RuleId, PROMPT_MARKER_RULE_ID};
use flight_state::AgentState;

/// Classify the screen: first matching rule (by priority) over the bottom
/// `lines_from_bottom` lines wins; otherwise the prompt marker, if configured and present
/// anywhere in the captured buffer, reads Idle. No match is `None`.
///
/// The marker scan covers the whole buffer, not just the rule window — preserved from
/// Fleet, where a stale prompt high in scrollback still reads Idle.
pub fn classify_screen(obs: &Observation, manifest: &Manifest) -> Option<Classification> {
    let skip = obs
        .screen_lines
        .len()
        .saturating_sub(manifest.lines_from_bottom());
    let window: Vec<&str> = obs
        .screen_lines
        .iter()
        .skip(skip)
        .map(String::as_str)
        .collect();
    let text = strip_ansi(&window.join("\n"));

    if let Some(rule) = manifest.screen_rules().iter().find(|r| r.is_match(&text)) {
        return Some(Classification {
            state: rule.state(),
            rule_id: rule.id().clone(),
        });
    }
    let marker = manifest.prompt_marker()?;
    obs.screen_lines
        .iter()
        .any(|l| l.contains(marker))
        .then(|| Classification {
            state: AgentState::Idle,
            rule_id: RuleId::new(PROMPT_MARKER_RULE_ID),
        })
}
