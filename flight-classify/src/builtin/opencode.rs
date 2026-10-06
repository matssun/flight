// SPDX-License-Identifier: MIT

//! OpenCode manifest: every state is scrape-sourced. Permit rules precede Busy (blocked
//! before working). No prompt marker: an unrecognized frame stays unclassified rather than
//! guessing Idle.

use crate::{AgentKind, ClassifyError, Manifest, Rule};
use flight_state::AgentState::{Busy, Permit};

pub(super) fn manifest() -> Result<Manifest, ClassifyError> {
    let screen = vec![
        Rule::new("permit.required", 10, r"△ Permission required", Permit)?,
        Rule::new(
            "permit.dismiss-confirm",
            20,
            r"esc dismiss.*(enter confirm|enter submit|enter toggle)|(enter confirm|enter submit|enter toggle).*esc dismiss",
            Permit,
        )?,
        Rule::new(
            "busy.esc-interrupt",
            30,
            r"esc to interrupt|ctrl\+c to interrupt|press esc to interrupt|esc again to interrupt",
            Busy,
        )?,
        // The block-character progress bar; four or more in a row so a stray box-drawing
        // character cannot false-positive.
        Rule::new("busy.progress-bar", 40, r"(■|⬝){4,}", Busy)?,
    ];
    Manifest::new(AgentKind::OpenCode, 15, None, screen, Vec::new())
}
