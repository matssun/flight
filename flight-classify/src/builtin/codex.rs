// SPDX-License-Identifier: MIT

//! Codex manifest. Native question controls come first so they are told apart from the
//! otherwise ambiguous "Action Required" title; `busy.esc-interrupt` is a hook-less
//! fallback and never overrides a permit rule above it. Blocked-title outranks
//! working-title.

use super::WORKING_TITLE;
use crate::{AgentKind, ClassifyError, Manifest, Rule};
use flight_state::AgentState::{Busy, Permit, Question};

pub(super) fn manifest() -> Result<Manifest, ClassifyError> {
    let screen = vec![
        Rule::new(
            "question.queued-follow-up",
            10,
            r"(?m)^[ \t]*• Queued follow-up inputs[ \t]*\n[ \t]*\? [1-9][0-9]* questions?(?:[ \t]+·[ \t]+[0-9]+[dhms](?:[ \t]+[0-9]+[dhms])*)?[ \t]*\n[ \t]*shift[ \t]*\+[ \t]*← to answer[ \t]*$",
            Question,
        )?,
        Rule::new(
            "question.submit-answer",
            20,
            r"(?m)^[ \t]*(?:tab to (?:add|edit) notes[ \t]*\|[ \t]*)?enter to submit (?:answer|all)\b[^\n]*(?:\n[ \t]+[^\n]*){0,2}\besc to interrupt[ \t]*$",
            Question,
        )?,
        Rule::new(
            "question.edit-notes",
            30,
            r"(?m)^[ \t]*tab or esc to [^|\n]+\|[ \t]*(?:\n[ \t]+)?enter to submit (?:answer|all)[ \t]*$",
            Question,
        )?,
        Rule::new(
            "question.async-answer",
            40,
            r"(?m)^[ \t]*enter submit\s+ctrl[ \t]*\+[ \t]*\] skip\s+(?:alt[ \t]*\+[ \t]*↓|shift[ \t]*\+[ \t]*→) main prompt(?:\s+shift[ \t]*\+[ \t]*← next question)?[ \t]*$",
            Question,
        )?,
        Rule::new("permit.allow", 50, r"(?i)allow command\?", Permit)?,
        Rule::new(
            "permit.confirm",
            60,
            r"(?i)press enter to confirm or esc to cancel",
            Permit,
        )?,
        Rule::new("permit.yn", 70, r"(?i)\[y/n\]", Permit)?,
        Rule::new("permit.do-you-want", 80, r"(?i)do you want to", Permit)?,
        Rule::new("busy.esc-interrupt", 90, r"(?i)esc to interrupt", Busy)?,
    ];
    let title = vec![
        Rule::new(
            "permit.title-action-required",
            10,
            r"Action Required",
            Permit,
        )?,
        Rule::new("busy.title-spinner", 20, WORKING_TITLE, Busy)?,
    ];
    Manifest::new(AgentKind::Codex, 15, Some("❯"), screen, title)
}
