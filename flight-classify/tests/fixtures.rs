// SPDX-License-Identifier: MIT

//! Real-capture corpus (from Fleet's src/state/fixtures, MIT; see THIRD_PARTY.md). A
//! file named `<agent>-<state>[-n].txt` must classify to that state; a `.title` sidecar
//! must too, for busy and permit fixtures of agents that have title rules.

use flight_classify::{classify_screen, classify_title, AgentKind, Manifest, Observation};
use flight_state::AgentState;
use std::fs;
use std::path::Path;

fn kind(name: &str) -> AgentKind {
    match name {
        "claude" => AgentKind::Claude,
        "codex" => AgentKind::Codex,
        "opencode" => AgentKind::OpenCode,
        "pi" => AgentKind::Pi,
        other => panic!("unknown agent in fixture name: {other}"),
    }
}

fn state(name: &str) -> AgentState {
    match name {
        "permit" => AgentState::Permit,
        "question" => AgentState::Question,
        "busy" => AgentState::Busy,
        "idle" => AgentState::Idle,
        other => panic!("unknown state in fixture name: {other}"),
    }
}

#[test]
fn every_fixture_classifies_to_its_filename_state() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut swept = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let mut parts = stem.split('-');
        let (agent, want) = (kind(parts.next().unwrap()), state(parts.next().unwrap()));
        let manifest = Manifest::builtin(agent).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let title = fs::read_to_string(path.with_extension("title")).unwrap_or_default();
        let obs = Observation {
            screen_lines: text.split('\n').map(str::to_owned).collect(),
            title: title.trim().to_owned(),
        };

        let got = classify_screen(&obs, &manifest).map(|c| c.state);
        assert_eq!(got, Some(want), "screen: {stem}");
        let has_title = !obs.title.is_empty() && !manifest.title_rules().is_empty();
        if has_title && matches!(want, AgentState::Busy | AgentState::Permit) {
            assert_eq!(
                classify_title(&obs, &manifest).map(|c| c.state),
                Some(want),
                "title: {stem}"
            );
        }
        swept += 1;
    }
    assert_eq!(swept, 7, "fixture corpus changed: swept {swept}");
}
