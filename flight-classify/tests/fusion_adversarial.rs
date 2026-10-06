// SPDX-License-Identifier: MIT

//! Overlapping, stale and contradictory evidence, with the provenance each outcome must
//! report. Cases marked "(Fleet behaviour)" document an existing Fleet precedence that is
//! kept for parity, not endorsed.

mod support;

use flight_classify::{fuse, Evidence, Reason, ScrapeVia, Source};
use flight_state::AgentState::{Busy, Done, Idle, Permit, Question};
use support::*;

const NOW: u64 = 1_000_000;

fn captured_at(ev: Evidence, ts: u64) -> Evidence {
    Evidence {
        screen_captured_at: Some(ts),
        ..ev
    }
}

#[test]
fn stale_hook_permit_is_cleared_by_a_live_bare_prompt() {
    let f = fuse(&with_screen(
        hooked("permit", NOW - 600, NOW),
        Idle,
        "idle.prompt",
    ));
    assert_eq!(
        (f.state, f.source, f.reason),
        (Idle, Source::Scrape, Reason::BarePromptClearedStalePrompt)
    );
    assert_eq!(
        f.candidates.hook,
        Some(Permit),
        "the losing hook reading is still reported"
    );
}

#[test]
fn stale_hook_working_is_overridden_by_a_live_counter_and_records_the_timeout() {
    let f = fuse(&with_screen(
        hooked("working", NOW - 600, NOW),
        Busy,
        "busy.token-counter-sec",
    ));
    assert_eq!((f.state, f.source), (Busy, Source::Scrape));
    assert!(f.working_timeout_fired);
}

#[test]
fn screen_older_than_the_hook_write_is_dropped() {
    // The hook says the turn finished at NOW-5; the screen was captured at NOW-10 and still
    // shows the permit prompt that has since been answered.
    let ev = captured_at(
        with_screen(hooked("done", NOW - 5, NOW), Permit, "permit.yn"),
        NOW - 10,
    );
    let f = fuse(&ev);
    assert_eq!((f.state, f.source), (Done, Source::Hook));
    assert_eq!(
        f.candidates.scrape, None,
        "a stale screen never enters the scrape slot"
    );
}

#[test]
fn screen_newer_than_the_hook_write_is_kept() {
    let ev = captured_at(
        with_screen(hooked("done", NOW - 10, NOW), Permit, "permit.yn"),
        NOW - 5,
    );
    assert_eq!(
        (fuse(&ev).state, fuse(&ev).source),
        (Permit, Source::Scrape)
    );
}

#[test]
fn screen_older_than_a_later_event_is_dropped_even_if_the_hook_is_old() {
    let ev = with_event(hooked("working", NOW - 100, NOW), Done, NOW - 3);
    let ev = captured_at(with_screen(ev, Question, "question.enter-select"), NOW - 50);
    let f = fuse(&ev);
    assert_eq!((f.state, f.source), (Done, Source::Event));
}

#[test]
fn live_screen_is_never_dropped() {
    let f = fuse(&with_screen(hooked("done", NOW, NOW), Permit, "permit.yn"));
    assert_eq!(f.state, Permit);
}

#[test]
fn current_title_beats_a_stale_busy_event() {
    // Event says Busy but it is 300s old, so it decays; a live working title still reads Busy.
    let ev = with_title(
        with_event(hooked("working", NOW - 300, NOW), Busy, NOW - 300),
        Busy,
        "busy.title-spinner",
    );
    let f = fuse(&ev);
    assert_eq!(
        (f.state, f.source, f.candidates.scrape_via),
        (Busy, Source::Scrape, Some(ScrapeVia::Title))
    );
    assert_eq!(f.rule_id().map(|r| r.as_str()), Some("busy.title-spinner"));
}

#[test]
fn silent_title_falls_back_to_the_screen() {
    let f = fuse(&with_screen(
        hooked("done", NOW, NOW),
        Busy,
        "busy.esc-interrupt",
    ));
    assert_eq!(f.candidates.scrape_via, Some(ScrapeVia::Screen));
}

#[test]
fn a_firing_title_takes_the_scrape_slot_over_a_live_screen_prompt() {
    // (Fleet behaviour) a non-codex title that fired shadows the screen in the scrape slot.
    let ev = with_screen(
        with_title(hooked("working", NOW, NOW), Busy, "busy.title-spinner"),
        Permit,
        "permit.yn",
    );
    let f = fuse(&ev);
    assert_eq!(
        (f.state, f.candidates.scrape_via),
        (Busy, Some(ScrapeVia::Title))
    );
}

#[test]
fn answered_question_with_a_lingering_event_is_cleared_by_the_bare_prompt() {
    let ev = with_screen(
        with_event(hooked("working", NOW - 30, NOW), Question, NOW - 20),
        Idle,
        "idle.prompt",
    );
    let f = fuse(&ev);
    assert_eq!(
        (f.state, f.reason),
        (Idle, Reason::BarePromptClearedStalePrompt)
    );
    assert_eq!(f.candidates.event, Some(Question));
}

#[test]
fn bare_prompt_never_demotes_a_finished_turn() {
    let f = fuse(&with_screen(
        with_event(hooked("working", NOW - 30, NOW), Done, NOW - 20),
        Idle,
        "idle.prompt",
    ));
    assert_eq!((f.state, f.source), (Done, Source::Event));
    assert_eq!(
        f.candidates.scrape.map(|c| c.state),
        Some(Idle),
        "the screen's reading is recorded"
    );
}

#[test]
fn weak_glyph_cannot_override_an_explicit_question_or_completion() {
    for derived in ["question", "done", "permit"] {
        let f = fuse(&with_screen(
            hooked(derived, NOW, NOW),
            Busy,
            "busy.spinner-glyph",
        ));
        assert_ne!(f.state, Busy, "{derived}");
        assert_eq!(f.source, Source::Hook, "{derived}");
    }
}

#[test]
fn strong_working_indicator_does_override_an_explicit_completion() {
    let f = fuse(&with_screen(
        hooked("done", NOW, NOW),
        Busy,
        "busy.token-counter-min",
    ));
    assert_eq!((f.state, f.reason), (Busy, Reason::LiveWorkingIndicator));
}

#[test]
fn stale_codex_question_screen_does_not_refine_the_action_required_title() {
    // The queued-question screen was captured before the latest hook write, so it is dropped
    // and the shared title stays a plain Permit.
    let ev = captured_at(
        codex_question_evidence(hooked("working", NOW - 2, NOW)),
        NOW - 10,
    );
    let f = fuse(&ev);
    assert_eq!(
        (f.state, f.candidates.scrape_via),
        (Permit, Some(ScrapeVia::Title))
    );
    assert_eq!(
        f.rule_id().map(|r| r.as_str()),
        Some("permit.title-action-required")
    );
}

#[test]
fn live_codex_question_screen_refines_the_title() {
    let ev = captured_at(
        codex_question_evidence(hooked("working", NOW - 20, NOW)),
        NOW - 10,
    );
    assert_eq!(fuse(&ev).state, Question);
}

#[test]
fn timestamps_in_the_future_do_not_underflow() {
    let f = fuse(&hooked("working", NOW + 1000, NOW));
    assert_eq!((f.state, f.working_timeout_fired), (Busy, false));
}

#[test]
fn no_evidence_is_idle_with_an_honest_reason() {
    let f = fuse(&unhooked(NOW));
    assert_eq!(
        (f.state, f.source, f.reason),
        (Idle, Source::Default, Reason::NoEvidence)
    );
}

#[test]
fn glyph_is_only_the_activity_source_when_there_is_no_event() {
    let with_ev = fuse(&glyph(with_event(unhooked(NOW), Done, NOW)));
    assert_eq!((with_ev.state, with_ev.source), (Done, Source::Event));
    let alone = fuse(&glyph(unhooked(NOW)));
    assert_eq!((alone.state, alone.source), (Busy, Source::Glyph));
}
