// SPDX-License-Identifier: MIT

//! Ported from the glyph debounce in Fleet's `discoverAgents` (src/agents/discovery.ts,
//! MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md).

use crate::AgentKind;

/// Whether the agent's screen glyph is a usable working signal. Codex paints ambient
/// braille particles around an idle composer, so its native working text and title rules
/// are the evidence instead.
pub(super) fn uses_screen_glyph(agent: AgentKind) -> bool {
    agent != AgentKind::Codex
}

/// Debounce the raw glyph: present means working and re-anchors; absent means still working
/// only while within `idle_secs` of the last glyph. The anchor is carried forward unchanged
/// while idle so the grace window stays tied to the last glyph actually seen (refreshing it
/// would stop a fast-sampled pane from ever expiring).
pub(super) fn debounce_glyph(
    agent: AgentKind,
    glyph_present: bool,
    anchor: Option<u64>,
    now: u64,
    idle_secs: u64,
) -> (bool, Option<u64>) {
    if !uses_screen_glyph(agent) {
        return (false, None);
    }
    if glyph_present {
        return (true, Some(now));
    }
    let working = anchor.is_some_and(|last| now.saturating_sub(last) < idle_secs);
    (working, anchor)
}
