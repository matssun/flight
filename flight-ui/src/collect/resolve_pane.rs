// SPDX-License-Identifier: MIT

use flight_classify::{
    classify_screen, classify_title, resolve, working_glyph_present, AgentKind, Evidence, Manifest,
    Observation, ResolveInput, ResolvedState, DEFAULT_IDLE_SECS,
};
use flight_control::{HostPane, HostRegistry};

/// Lines captured per pane for classification (Fleet's scrape window).
const SCRAPE_LINES: u32 = 50;

/// Observe one agent pane (capture its screen), classify it with its agent's manifest, and
/// resolve its state against the previous resolution. A pane whose capture fails is resolved
/// with no screen evidence rather than dropped.
pub(super) fn resolve_pane(
    registry: &HostRegistry,
    pane: &HostPane,
    agent: AgentKind,
    previous: Option<&ResolvedState>,
    now: u64,
) -> ResolvedState {
    let screen_lines: Vec<String> = registry
        .capture_pane(&pane.pane_ref, true, Some(SCRAPE_LINES))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    let obs = Observation {
        screen_lines,
        title: pane.info.pane_title.clone(),
    };
    let evidence = match Manifest::builtin(agent) {
        Ok(m) => Evidence {
            screen: classify_screen(&obs, &m),
            title: classify_title(&obs, &m),
            focused: pane.info.focused,
            ..Evidence::empty(agent, now)
        },
        Err(_) => Evidence {
            focused: pane.info.focused,
            ..Evidence::empty(agent, now)
        },
    };
    resolve(&ResolveInput {
        previous,
        evidence: &evidence,
        glyph_seen: working_glyph_present(&obs.screen_lines),
        idle_secs: DEFAULT_IDLE_SECS,
    })
}
