// SPDX-License-Identifier: MIT

//! Claude manifest. Ordering is deliberate, in three tiers:
//! 1. Live-only BUSY rules (token counter, esc-to-interrupt) first: they render only while
//!    a turn runs and vanish when a dialog is up, so they safely outrank PERMIT/QUESTION.
//!    An answered prompt still in the bottom window must read Busy, not a false Permit.
//! 2. PERMIT/QUESTION prompt rules. A genuine dialog suspends the counter, so tier 1 never
//!    shadows a real prompt.
//! 3. The braille glyph last: a weaker signal (it can appear quoted in transcript text), so
//!    a glyph beside a real `[y/n]` still reads Permit.

use super::{WORKING_GLYPH, WORKING_TITLE};
use crate::{AgentKind, ClassifyError, Manifest, Rule};
use flight_state::AgentState::{Busy, Permit, Question};

pub(super) fn manifest() -> Result<Manifest, ClassifyError> {
    let screen = vec![
        Rule::new(
            "busy.token-counter-min",
            10,
            r"\([0-9]+m\s+[0-9]+s\s+·.*tokens?\)",
            Busy,
        )?,
        Rule::new(
            "busy.token-counter-sec",
            20,
            r"\([0-9]+s\s+·.*tokens?\)",
            Busy,
        )?,
        Rule::new(
            "busy.spinner-elapsed",
            30,
            r"…[ \t]+\((?:[^()\n]*[ \t]·[ \t]+)?(?:[0-9]+h[ \t]+)?(?:[0-9]+m[ \t]+)?[0-9]+s(?:[ \t]+·[ \t][^()\n]*)?\)",
            Busy,
        )?,
        Rule::new("busy.esc-interrupt", 40, r"(?i)esc to interrupt", Busy)?,
        Rule::new("permit.yn", 50, r"(?i)\[y/n\]|\[Y/n\]", Permit)?,
        Rule::new(
            "permit.do-you-want",
            60,
            r"Do you want to (proceed|allow)",
            Permit,
        )?,
        Rule::new(
            "permit.waiting-for-permission",
            70,
            r"(?i)waiting for permission",
            Permit,
        )?,
        Rule::new(
            "permit.allow-connection",
            80,
            r"(?i)do you want to allow this connection\?",
            Permit,
        )?,
        Rule::new("permit.tab-to-amend", 90, r"(?i)tab to amend", Permit)?,
        Rule::new(
            "permit.ctrl-e-explain",
            100,
            r"(?i)ctrl\+e to explain",
            Permit,
        )?,
        Rule::new(
            "permit.dynamic-workflow",
            110,
            r"(?i)run a dynamic workflow\?",
            Permit,
        )?,
        Rule::new(
            "question.enter-select",
            120,
            r"Enter to select.*[↑↓]|Esc to cancel",
            Question,
        )?,
        Rule::new("busy.spinner-glyph", 130, WORKING_GLYPH, Busy)?,
    ];
    // Claude paints dingbat spinners on screen but a braille frame in the title while
    // working, so the title is the reliable fast signal for a hook-less claude.
    let title = vec![Rule::new("busy.title-spinner", 10, WORKING_TITLE, Busy)?];
    Manifest::new(AgentKind::Claude, 15, Some("❯"), screen, title)
}
