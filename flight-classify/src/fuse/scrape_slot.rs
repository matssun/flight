// SPDX-License-Identifier: MIT

//! Which scrape evidence enters the fusion. Ported from the hooked-agent branch of Fleet's
//! `refreshStates` (src/state/refresh.ts, MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md).

use super::fused::ScrapeVia;
use super::Evidence;
use crate::{refine_title_with_screen, Classification};

/// The title wins the scrape slot when it fired (it is re-read every tick, so fresher and
/// cheaper than a screen capture); the screen covers the ticks where the title is silent.
/// A codex "Action Required" title is first refined by a queued-question screen. A stale
/// screen (older than the newest hook/event) is dropped before either step.
pub(super) fn scrape_slot(ev: &Evidence) -> Option<(Classification, ScrapeVia)> {
    let screen = live_screen(ev);
    match ev.title.clone() {
        Some(t) => Some((
            refine_title_with_screen(ev.agent, t, screen),
            ScrapeVia::Title,
        )),
        None => screen.map(|s| (s, ScrapeVia::Screen)),
    }
}

fn live_screen(ev: &Evidence) -> Option<Classification> {
    let newest_signal = ev
        .hook
        .map(|h| h.ts)
        .into_iter()
        .chain(ev.event.map(|e| e.ts))
        .max();
    let stale = match (ev.screen_captured_at, newest_signal) {
        (Some(captured), Some(signal)) => signal > captured,
        _ => false,
    };
    if stale {
        None
    } else {
        ev.screen.clone()
    }
}
