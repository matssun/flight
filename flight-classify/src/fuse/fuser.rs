// SPDX-License-Identifier: MIT

//! Ported from Fleet's `fuseState` and `fuseDiscoveredState` (src/state/engine.ts, MIT,
//! (c) 2026 Nick Nisi; see THIRD_PARTY.md). Precedence is the sequence of steps in `fuse`,
//! each a named function with its own tests.

use super::fused::{Candidates, Reason, Source};
use super::{scrape_slot::scrape_slot, Evidence, FusedClassification};
use crate::Classification;
use flight_state::{needs_attention, AgentState};

/// A working state with no activity for this long decays to Idle.
pub const WORKING_TIMEOUT_SECS: u64 = 180;

const SPINNER_GLYPH_RULE: &str = "busy.spinner-glyph";

/// What the hook/event layers (or the glyph) say, before the screen is consulted.
struct Derived {
    state: AgentState,
    source: Source,
    reason: Reason,
    timed_out: bool,
}

pub fn fuse(ev: &Evidence) -> FusedClassification {
    let slot = scrape_slot(ev);
    let candidates = Candidates {
        hook: ev.hook.map(|h| h.state),
        event: ev.event.map(|e| e.state),
        scrape: slot.as_ref().map(|(c, _)| c.clone()),
        scrape_via: slot.as_ref().map(|(_, v)| *v),
    };
    let scrape = slot.as_ref().map(|(c, _)| c);
    let done = |state, source, reason, timed_out| FusedClassification {
        state,
        source,
        reason,
        candidates: candidates.clone(),
        working_timeout_fired: timed_out,
    };

    if let Some(state) = trusted_prompt(scrape) {
        return done(state, Source::Scrape, Reason::PromptOnScreen, false);
    }
    let derived = derive(ev);
    // The timeout flag records that the derived layer decayed, even when the screen then
    // overrides it (as Fleet's decision trace does).
    if live_working_overrides(scrape, derived.state) {
        return done(
            AgentState::Busy,
            Source::Scrape,
            Reason::LiveWorkingIndicator,
            derived.timed_out,
        );
    }
    if bare_prompt_clears_stale_prompt(scrape, derived.state) {
        return done(
            AgentState::Idle,
            Source::Scrape,
            Reason::BarePromptClearedStalePrompt,
            derived.timed_out,
        );
    }
    done(
        derived.state,
        derived.source,
        derived.reason,
        derived.timed_out,
    )
}

/// Step 1: the scraper reliably reads permission prompts and question dialogs, which the
/// hook layer cannot tell apart. Trusted absolutely.
fn trusted_prompt(scrape: Option<&Classification>) -> Option<AgentState> {
    scrape
        .map(|c| c.state)
        .filter(|s| matches!(s, AgentState::Permit | AgentState::Question))
}

/// Step 2: the reliable activity signal from hook/event/glyph, with working-timeout decay
/// anchored to the freshest activity timestamp.
fn derive(ev: &Evidence) -> Derived {
    let (state, source, reason) = if let Some(e) = ev.event {
        (e.state, Source::Event, Reason::LatestEvent)
    } else if ev.working_glyph {
        (AgentState::Busy, Source::Glyph, Reason::ActivityGlyph)
    } else if let Some(h) = ev.hook {
        (h.state, Source::Hook, Reason::HookStatus)
    } else {
        (AgentState::Idle, Source::Default, Reason::NoEvidence)
    };
    // A hook-less pane or glyph has no timestamp of its own: treat it as fresh.
    let hook_ts = ev.hook.map_or(ev.now, |h| h.ts);
    let last_activity = hook_ts.max(ev.event.map_or(0, |e| e.ts));
    let age = ev.now.saturating_sub(last_activity);
    if state == AgentState::Busy && age >= WORKING_TIMEOUT_SECS {
        return Derived {
            state: AgentState::Idle,
            source: Source::Default,
            reason: Reason::WorkingTimedOut,
            timed_out: true,
        };
    }
    Derived {
        state,
        source,
        reason,
        timed_out: false,
    }
}

/// Step 3: a live Busy read from the scrape beats the derived state, except that the weak
/// braille-glyph rule cannot override an explicit question/permit/completion.
fn live_working_overrides(scrape: Option<&Classification>, derived: AgentState) -> bool {
    let Some(c) = scrape else { return false };
    let protected = c.rule_id.as_str() == SPINNER_GLYPH_RULE && needs_attention(derived);
    c.state == AgentState::Busy && !protected
}

/// Step 4: a bare prompt can only retire a stale permit/question. It never overrides a
/// derived Done or Busy; time decay does that.
fn bare_prompt_clears_stale_prompt(scrape: Option<&Classification>, derived: AgentState) -> bool {
    scrape.is_some_and(|c| c.state == AgentState::Idle)
        && matches!(derived, AgentState::Permit | AgentState::Question)
}
