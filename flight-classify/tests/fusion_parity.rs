// SPDX-License-Identifier: MIT

//! Fusion parity: cases ported from Fleet's engine.test.ts (MIT, (c) 2026 Nick Nisi;
//! see THIRD_PARTY.md), plus the Codex fusion case from codex-detection.test.ts.

mod support;

use flight_classify::{fuse, Reason, Source, WORKING_TIMEOUT_SECS};
use flight_state::AgentState::{Busy, Done, Idle, Permit, Question};
use support::*;

const NOW: u64 = 1_000_000;

#[test]
fn fresh_hook_wins() {
    let f = fuse(&hooked("working", NOW, NOW));
    assert_eq!((f.state, f.source), (Busy, Source::Hook));
}

#[test]
fn event_overrides_hook_when_more_specific() {
    let f = fuse(&with_event(hooked("completed", NOW, NOW), Busy, NOW));
    assert_eq!((f.state, f.source), (Busy, Source::Event));
}

#[test]
fn scrape_permit_always_wins() {
    let ev = with_screen(
        with_event(hooked("working", NOW, NOW), Busy, NOW),
        Permit,
        "permit.yn",
    );
    let f = fuse(&ev);
    assert_eq!(
        (f.state, f.source, f.reason),
        (Permit, Source::Scrape, Reason::PromptOnScreen)
    );
    assert_eq!(f.rule_id().map(|r| r.as_str()), Some("permit.yn"));
}

#[test]
fn scrape_permit_overrides_hook_busy_with_no_event() {
    let f = fuse(&with_screen(
        hooked("working", NOW, NOW),
        Permit,
        "permit.yn",
    ));
    assert_eq!((f.state, f.source), (Permit, Source::Scrape));
}

#[test]
fn done_never_auto_decays() {
    assert_eq!(fuse(&hooked("completed", NOW - 3600, NOW)).state, Done);
}

#[test]
fn waiting_maps_to_permit() {
    assert_eq!(fuse(&hooked("waiting", NOW, NOW)).state, Permit);
}

#[test]
fn scrape_idle_does_not_override_a_fresh_working_hook() {
    let f = fuse(&with_screen(
        hooked("working", NOW, NOW),
        Idle,
        "idle.prompt",
    ));
    assert_eq!(f.state, Busy);
}

#[test]
fn scrape_idle_clears_a_stale_permit() {
    let f = fuse(&with_screen(
        hooked("waiting", NOW - 5, NOW),
        Idle,
        "idle.prompt",
    ));
    assert_eq!(
        (f.state, f.source, f.reason),
        (Idle, Source::Scrape, Reason::BarePromptClearedStalePrompt)
    );
}

#[test]
fn scrape_busy_wins_over_an_idle_hook() {
    let f = fuse(&with_screen(
        hooked("done", NOW, NOW),
        Busy,
        "busy.token-counter-sec",
    ));
    assert_eq!((f.state, f.source), (Busy, Source::Scrape));
}

#[test]
fn scrape_idle_does_not_override_a_fresh_done() {
    let f = fuse(&with_screen(
        hooked("completed", NOW, NOW),
        Idle,
        "idle.prompt",
    ));
    assert_eq!(f.state, Done);
}

#[test]
fn working_hook_past_the_timeout_decays_to_idle() {
    let f = fuse(&hooked("working", NOW - 200, NOW));
    assert_eq!(
        (f.state, f.source, f.working_timeout_fired),
        (Idle, Source::Default, true)
    );
    assert_eq!(f.reason, Reason::WorkingTimedOut);
}

#[test]
fn timeout_boundary_is_inclusive() {
    let at = fuse(&hooked("working", NOW - WORKING_TIMEOUT_SECS, NOW));
    let before = fuse(&hooked("working", NOW - WORKING_TIMEOUT_SECS + 1, NOW));
    assert_eq!((at.state, before.state), (Idle, Busy));
}

#[test]
fn fresh_event_busy_is_not_decayed_by_a_stale_hook_ts() {
    let f = fuse(&with_event(
        hooked("working", NOW - 200, NOW),
        Busy,
        NOW - 10,
    ));
    assert_eq!((f.state, f.working_timeout_fired), (Busy, false));
}

#[test]
fn event_busy_with_a_stale_event_ts_still_decays() {
    let f = fuse(&with_event(
        hooked("working", NOW - 300, NOW),
        Busy,
        NOW - 200,
    ));
    assert_eq!((f.state, f.working_timeout_fired), (Idle, true));
}

// --- Fleet's fuseDiscoveredState: no hook layer, glyph as the activity signal ---

#[test]
fn discovered_agents() {
    let none = || unhooked(NOW);
    assert_eq!(
        fuse(&with_screen(none(), Permit, "permit.yn")).state,
        Permit
    );
    assert_eq!(
        fuse(&with_screen(none(), Question, "question.x")).state,
        Question
    );
    assert_eq!(
        fuse(&with_screen(none(), Busy, "busy.token-counter-sec")).state,
        Busy
    );
    assert_eq!(fuse(&glyph(none())).state, Busy, "glyph alone reads Busy");
    assert_eq!(
        fuse(&with_screen(glyph(none()), Idle, "idle.prompt")).state,
        Busy,
        "a bare prompt never demotes a live glyph"
    );
    assert_eq!(fuse(&none()).state, Idle);
    assert_eq!(fuse(&with_screen(none(), Idle, "idle.prompt")).state, Idle);
}

// --- Codex: an Action Required title refined by a queued-question screen ---

#[test]
fn codex_action_required_title_refined_to_question_through_the_fusion() {
    let ev = codex_question_evidence(hooked("working", NOW, NOW));
    let f = fuse(&ev);
    assert_eq!((f.state, f.source), (Question, Source::Scrape));
    assert_eq!(
        f.rule_id().map(|r| r.as_str()),
        Some("question.queued-follow-up")
    );
}
