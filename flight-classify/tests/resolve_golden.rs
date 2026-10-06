// SPDX-License-Identifier: MIT

//! Differential parity for the stateful tier. `tests/golden/resolve.csv` and `glyph.csv` are
//! Fleet's own `resolveDiscoveredStatus` and `discoverAgents` (MIT, (c) 2026 Nick Nisi)
//! driven over signal sequences by `tools/gen_resolve_golden.mjs`. Flight must produce the
//! same state at every step of every sequence.

mod support;

use flight_classify::{resolve, AgentKind, Evidence, ResolveInput, ResolvedState, Source};
use flight_state::AgentState;
use support::*;

fn code(c: char, permit_rule: &str) -> Option<flight_classify::Classification> {
    match c {
        '-' => None,
        'P' => Some(cls(AgentState::Permit, permit_rule)),
        'Q' => Some(cls(AgentState::Question, "question.enter-select")),
        'B' => Some(cls(AgentState::Busy, "busy.token-counter-sec")),
        'I' => Some(cls(AgentState::Idle, "idle.prompt")),
        other => panic!("bad signal code {other}"),
    }
}

fn letter(s: AgentState) -> char {
    match s {
        AgentState::Permit => 'P',
        AgentState::Question => 'Q',
        AgentState::Busy => 'B',
        AgentState::Idle => 'I',
        AgentState::Done => 'D',
        other => panic!("unexpected state {other:?}"),
    }
}

#[test]
fn done_machine_agrees_with_fleet_on_every_sequence() {
    let csv = include_str!("golden/resolve.csv");
    let (mut rows, mut with_done, mut bad) = (0, 0, Vec::new());
    for line in csv.lines().filter(|l| !l.is_empty()) {
        let (seq, want) = line.split_once(';').unwrap();
        let mut prev: Option<ResolvedState> = None;
        let mut got = String::new();
        for (i, tok) in seq.split(' ').enumerate() {
            let c: Vec<char> = tok.chars().collect();
            let ev = Evidence {
                title: code(c[1], "permit.title-action-required"),
                screen: code(c[2], "permit.yn"),
                focused: c[3] == '1',
                ..unhooked(100 + i as u64)
            };
            // idle_secs 0 turns the debounce into "glyph present = working", matching the
            // already-debounced glyphWorking signal Fleet's function takes.
            let r = resolve(&ResolveInput {
                previous: prev.as_ref(),
                evidence: &ev,
                glyph_seen: c[0] == '1',
                idle_secs: 0,
            });
            got.push(letter(r.state));
            prev = Some(r);
        }
        rows += 1;
        with_done += usize::from(want.contains('D'));
        if got != want {
            bad.push(format!("{line} -> {got}"));
        }
    }
    assert_eq!(rows, 5160, "golden size changed");
    assert!(with_done > 300, "golden barely exercises Done: {with_done}");
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn glyph_debounce_agrees_with_fleet_on_every_timed_sequence() {
    let csv = include_str!("golden/glyph.csv");
    let (mut rows, mut bad) = (0, Vec::new());
    for line in csv.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split(';').collect();
        let agent = if f[0] == "codex" {
            AgentKind::Codex
        } else {
            AgentKind::Other
        };
        let (mut prev, mut now, mut got) = (None::<ResolvedState>, 1000u64, String::new());
        for st in f[1].split(' ') {
            now += u64::from(st.as_bytes()[0] - b'0');
            let ev = Evidence::empty(agent, now);
            let r = resolve(&ResolveInput {
                previous: prev.as_ref(),
                evidence: &ev,
                glyph_seen: st.ends_with('1'),
                idle_secs: 3,
            });
            got.push(if r.provenance.fused.source == Source::Glyph {
                'W'
            } else {
                'i'
            });
            prev = Some(r);
        }
        rows += 1;
        if got != f[2] {
            bad.push(format!("{line} -> {got}"));
        }
    }
    assert_eq!(rows, 2728, "golden size changed");
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}
