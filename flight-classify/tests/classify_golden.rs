// SPDX-License-Identifier: MIT

//! Differential parity: `tests/golden/classify.csv` is Fleet's own detector output
//! (MIT, (c) 2026 Nick Nisi) over every 1-6 line window of every fixture capture under every
//! built-in manifest, plus every fixture title (see `tools/gen_classify_golden.mjs`). Flight
//! must produce the same state and rule id for each row, which checks the JS-to-Rust regex
//! translation on varied real input.

use flight_classify::{
    classify_screen, classify_title, AgentKind, Classification, Manifest, Observation,
};
use flight_state::AgentState;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn agent(name: &str) -> AgentKind {
    match name {
        "claude" => AgentKind::Claude,
        "codex" => AgentKind::Codex,
        "opencode" => AgentKind::OpenCode,
        "pi" => AgentKind::Pi,
        other => panic!("unknown agent {other}"),
    }
}

fn want(status: &str, rule: &str) -> Option<(AgentState, String)> {
    let state = match status {
        "-" => return None,
        "PERMIT" => AgentState::Permit,
        "QUESTION" => AgentState::Question,
        "BUSY" => AgentState::Busy,
        "IDLE" => AgentState::Idle,
        other => panic!("unknown status {other}"),
    };
    Some((state, rule.to_owned()))
}

fn pair(c: Option<Classification>) -> Option<(AgentState, String)> {
    c.map(|c| (c.state, c.rule_id.to_string()))
}

#[test]
fn flight_agrees_with_fleet_on_every_window_and_title() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = include_str!("golden/classify.csv");
    let mut manifests: HashMap<String, Manifest> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();
    let (mut rows, mut matched, mut mismatches) = (0, 0, Vec::new());
    for line in csv.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        let manifest = manifests
            .entry(f[1].to_owned())
            .or_insert_with(|| Manifest::builtin(agent(f[1])).unwrap());
        let text = texts
            .entry(f[2].to_owned())
            .or_insert_with(|| fs::read_to_string(dir.join(f[2])).unwrap());
        let (got, expected) = if f[0] == "S" {
            let (start, len): (usize, usize) = (f[3].parse().unwrap(), f[4].parse().unwrap());
            let lines: Vec<String> = text
                .split('\n')
                .skip(start)
                .take(len)
                .map(str::to_owned)
                .collect();
            let obs = Observation {
                screen_lines: lines,
                title: String::new(),
            };
            (pair(classify_screen(&obs, manifest)), want(f[5], f[6]))
        } else {
            let obs = Observation {
                screen_lines: Vec::new(),
                title: text.trim().to_owned(),
            };
            (pair(classify_title(&obs, manifest)), want(f[5], f[6]))
        };
        rows += 1;
        if expected.is_some() {
            matched += 1;
        }
        if got != expected {
            mismatches.push(format!("{line}: got {got:?}"));
        }
    }
    assert!(rows > 2000, "golden too small: {rows}");
    assert!(
        matched > 100,
        "golden exercises too few positive matches: {matched}"
    );
    assert!(
        mismatches.is_empty(),
        "{} mismatches, first: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(5)]
    );
}
