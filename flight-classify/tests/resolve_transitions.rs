// SPDX-License-Identifier: MIT

//! Explicit transitions over time for the stateful tier. Scenarios follow Fleet's
//! `resolveDiscoveredStatus` DONE-synthesis tests (src/agents/discovery.test.ts, MIT,
//! (c) 2026 Nick Nisi); the cold-start, provenance and hooked-path cases are Flight's.

mod support;

use flight_classify::{
    prune_tracking, resolve, AgentKind, Evidence, ResolveInput, ResolvedState, Source,
};
use flight_state::AgentState::{self, Busy, Done, Idle, Permit, Question};
use std::collections::{HashMap, HashSet};
use support::*;

/// One pane's timeline: each `tick` resolves against the previous result.
struct Pane {
    agent: AgentKind,
    now: u64,
    prev: Option<ResolvedState>,
}

impl Pane {
    fn new(agent: AgentKind) -> Self {
        Self {
            agent,
            now: 1000,
            prev: None,
        }
    }

    fn tick(
        &mut self,
        dt: u64,
        glyph: bool,
        screen: Option<(AgentState, &str)>,
        focused: bool,
    ) -> AgentState {
        self.now += dt;
        let ev = Evidence {
            screen: screen.map(|(s, r)| cls(s, r)),
            focused,
            ..Evidence::empty(self.agent, self.now)
        };
        let r = resolve(&ResolveInput {
            previous: self.prev.as_ref(),
            evidence: &ev,
            glyph_seen: glyph,
            idle_secs: 3,
        });
        let state = r.state;
        self.prev = Some(r);
        state
    }

    fn busy(&mut self, focused: bool) -> AgentState {
        self.tick(1, false, Some((Busy, "busy.token-counter-sec")), focused)
    }

    fn idle(&mut self, focused: bool) -> AgentState {
        self.tick(10, false, Some((Idle, "idle.prompt")), focused)
    }

    fn done_flag(&self) -> bool {
        self.prev
            .as_ref()
            .is_some_and(|p| p.provenance.synthesized_done)
    }
}

fn claude() -> Pane {
    Pane::new(AgentKind::Claude)
}

#[test]
fn busy_then_idle_while_away_reads_done_and_persists() {
    let mut p = claude();
    assert_eq!(p.busy(false), Busy);
    assert_eq!(p.idle(false), Done);
    for _ in 0..50 {
        assert_eq!(p.idle(false), Done, "Done never decays with time");
    }
}

#[test]
fn busy_then_idle_while_watching_reads_idle() {
    let mut p = claude();
    p.busy(true);
    assert_eq!(p.idle(true), Idle, "you watched it finish");
    assert_eq!(p.idle(false), Idle, "and it does not become Done later");
}

#[test]
fn viewing_the_pane_clears_a_pending_done() {
    let mut p = claude();
    p.busy(false);
    assert_eq!(p.idle(false), Done);
    assert_eq!(p.idle(true), Idle);
    assert_eq!(p.idle(false), Idle, "acknowledged for good");
}

#[test]
fn work_resuming_clears_done_and_rearms_the_transition() {
    let mut p = claude();
    p.busy(false);
    assert_eq!(p.idle(false), Done);
    assert_eq!(p.busy(false), Busy);
    assert_eq!(
        p.idle(false),
        Done,
        "a second finished turn is a second Done"
    );
}

#[test]
fn a_prompt_holds_the_transition_open() {
    let mut p = claude();
    p.busy(false);
    assert_eq!(p.tick(1, false, Some((Permit, "permit.yn")), false), Permit);
    assert_eq!(p.idle(false), Done, "answering the prompt ended the turn");
}

#[test]
fn a_prompt_with_no_busy_history_also_lands_on_done() {
    let mut p = claude();
    assert_eq!(
        p.tick(1, false, Some((Question, "question.enter-select")), false),
        Question
    );
    assert_eq!(p.idle(false), Done);
}

#[test]
fn a_never_busy_pane_is_plain_idle() {
    for focused in [false, true] {
        let mut p = claude();
        assert_eq!(p.idle(focused), Idle);
        assert_eq!(p.idle(focused), Idle);
    }
}

#[test]
fn panes_are_tracked_independently() {
    let (mut a, mut b) = (claude(), claude());
    a.busy(false);
    assert_eq!(a.idle(false), Done);
    assert_eq!(b.idle(false), Idle);
}

#[test]
fn a_cold_start_never_synthesizes_done() {
    // Fleet: one-shot invocations start cold and never synthesize Done. After a restart or
    // reconnect, `previous: None` forgets a pending Done. Who owns Tracking decides that.
    let mut p = claude();
    p.busy(false);
    assert_eq!(p.idle(false), Done);
    p.prev = None;
    assert_eq!(p.idle(false), Idle);
}

#[test]
fn synthesized_done_is_reported_as_such_with_the_fusion_underneath() {
    let mut p = claude();
    p.busy(false);
    assert!(!p.done_flag());
    assert_eq!(p.idle(false), Done);
    assert!(p.done_flag());
    let prov = &p.prev.as_ref().unwrap().provenance;
    assert_eq!(prov.fused.state, Idle, "the evidence itself said Idle");
}

#[test]
fn glyph_grace_expires_after_idle_secs_anchored_to_the_last_glyph() {
    let mut p = Pane::new(AgentKind::Other);
    assert_eq!(p.tick(1, true, None, false), Busy);
    assert_eq!(p.tick(1, false, None, false), Busy, "within grace");
    assert_eq!(
        p.tick(1, false, None, false),
        Busy,
        "still within grace of the last glyph"
    );
    assert_eq!(
        p.tick(1, false, None, false),
        Done,
        "grace expired: the finished turn is Done"
    );
    assert_eq!(p.prev.as_ref().unwrap().tracking.glyph_anchor, Some(1001));
}

#[test]
fn codex_ignores_the_screen_glyph() {
    let mut p = Pane::new(AgentKind::Codex);
    assert_eq!(
        p.tick(1, true, None, false),
        Idle,
        "ambient particles are not work"
    );
    assert_eq!(p.prev.as_ref().unwrap().tracking.glyph_anchor, None);
    assert_eq!(
        p.tick(1, true, Some((Busy, "busy.esc-interrupt")), false),
        Busy,
        "native evidence still counts"
    );
}

#[test]
fn hooked_panes_are_stateless_and_never_synthesize_done() {
    let mut prev: Option<ResolvedState> = None;
    for now in [1000, 1001, 1002] {
        let ev = Evidence {
            focused: false,
            ..hooked("working", now, now)
        };
        let r = resolve(&ResolveInput {
            previous: prev.as_ref(),
            evidence: &ev,
            glyph_seen: false,
            idle_secs: 3,
        });
        assert_eq!((r.state, r.provenance.synthesized_done), (Busy, false));
        prev = Some(r);
    }
    let ev = Evidence {
        focused: false,
        ..hooked("done", 1003, 1003)
    };
    let r = resolve(&ResolveInput {
        previous: prev.as_ref(),
        evidence: &ev,
        glyph_seen: false,
        idle_secs: 3,
    });
    assert_eq!(
        (r.state, r.provenance.fused.source),
        (Done, Source::Hook),
        "Done comes from the hook, not the machine"
    );
    assert_eq!(
        r.tracking,
        prev.unwrap().tracking,
        "hooked resolution leaves tracking untouched"
    );
}

#[test]
fn prune_drops_tracking_for_vanished_panes() {
    let mut map: HashMap<&str, u8> = HashMap::from([("%1", 1), ("%2", 2), ("%3", 3)]);
    prune_tracking(&mut map, &HashSet::from(["%2"]));
    assert_eq!(map.keys().copied().collect::<Vec<_>>(), ["%2"]);
}

#[test]
fn unknown_agents_have_an_empty_manifest() {
    use flight_classify::{classify_screen, classify_title, Manifest, Observation};
    let m = Manifest::builtin(AgentKind::Other).unwrap();
    let obs = Observation {
        screen_lines: vec![
            "Do you want to proceed? [y/n]".into(),
            "esc to interrupt".into(),
        ],
        title: "Action Required".into(),
    };
    assert_eq!(
        (classify_screen(&obs, &m), classify_title(&obs, &m)),
        (None, None)
    );
}
