// SPDX-License-Identifier: MIT

//! Evidence builders shared by the fusion test files.
#![allow(dead_code)]

use flight_classify::{
    classify_screen, classify_title, AgentKind, Classification, EventObservation, Evidence,
    HookObservation, Manifest, Observation, RuleId,
};
use flight_state::AgentState;

pub fn unhooked(now: u64) -> Evidence {
    Evidence::empty(AgentKind::Claude, now)
}

pub fn hooked(state: &str, hook_ts: u64, now: u64) -> Evidence {
    Evidence {
        hook: Some(HookObservation::from_wire(state, hook_ts)),
        ..unhooked(now)
    }
}

pub fn with_event(ev: Evidence, state: AgentState, ts: u64) -> Evidence {
    Evidence {
        event: Some(EventObservation { state, ts }),
        ..ev
    }
}

pub fn cls(state: AgentState, rule: &str) -> Classification {
    Classification {
        state,
        rule_id: RuleId::new(rule),
    }
}

pub fn with_screen(ev: Evidence, state: AgentState, rule: &str) -> Evidence {
    Evidence {
        screen: Some(cls(state, rule)),
        ..ev
    }
}

pub fn with_title(ev: Evidence, state: AgentState, rule: &str) -> Evidence {
    Evidence {
        title: Some(cls(state, rule)),
        ..ev
    }
}

pub fn glyph(ev: Evidence) -> Evidence {
    Evidence {
        working_glyph: true,
        ..ev
    }
}

/// Codex with the real queued-question screen and the shared "Action Required" title.
pub fn codex_question_evidence(base: Evidence) -> Evidence {
    let m = Manifest::builtin(AgentKind::Codex).unwrap();
    let lines = [
        "• Working (3m 46s • esc to interrupt)",
        "",
        "• Queued follow-up inputs",
        "  ? 1 question",
        "    shift + ← to answer",
        "",
        "› Ask Codex to do anything",
    ];
    let obs = Observation {
        screen_lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        title: "Action Required | Codex".to_owned(),
    };
    Evidence {
        agent: AgentKind::Codex,
        screen: classify_screen(&obs, &m),
        title: classify_title(&obs, &m),
        ..base
    }
}
