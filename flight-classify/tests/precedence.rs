// SPDX-License-Identifier: MIT

//! Rule ordering is explicit data: priority decides, never construction order.

use flight_classify::{classify_screen, AgentKind, ClassifyError, Manifest, Observation, Rule};
use flight_state::AgentState::{Busy, Permit};

const BOTH: &str = "Do you want to proceed? (8s · ↑ 240 tokens)";
const PERMIT_PAT: &str = r"Do you want to (proceed|allow)";
const BUSY_PAT: &str = r"\([0-9]+s\s+·.*tokens?\)";

fn obs() -> Observation {
    Observation {
        screen_lines: vec![BOTH.to_owned()],
        title: String::new(),
    }
}

fn manifest(rules: Vec<Rule>) -> Manifest {
    Manifest::new(AgentKind::Claude, 15, None, rules, Vec::new()).unwrap()
}

#[test]
fn lower_priority_number_wins_when_several_rules_match() {
    let m = manifest(vec![
        Rule::new("p", 1, PERMIT_PAT, Permit).unwrap(),
        Rule::new("b", 2, BUSY_PAT, Busy).unwrap(),
    ]);
    assert_eq!(classify_screen(&obs(), &m).unwrap().rule_id.as_str(), "p");
}

#[test]
fn construction_order_does_not_matter() {
    let m = manifest(vec![
        Rule::new("b", 2, BUSY_PAT, Busy).unwrap(),
        Rule::new("p", 1, PERMIT_PAT, Permit).unwrap(),
    ]);
    assert_eq!(classify_screen(&obs(), &m).unwrap().rule_id.as_str(), "p");
}

#[test]
fn swapping_priorities_swaps_the_winner() {
    let m = manifest(vec![
        Rule::new("p", 2, PERMIT_PAT, Permit).unwrap(),
        Rule::new("b", 1, BUSY_PAT, Busy).unwrap(),
    ]);
    let c = classify_screen(&obs(), &m).unwrap();
    assert_eq!((c.state, c.rule_id.as_str()), (Busy, "b"));
}

#[test]
fn ambiguous_ordering_is_rejected() {
    let dup_priority = Manifest::new(
        AgentKind::Claude,
        15,
        None,
        vec![
            Rule::new("a", 1, "a", Busy).unwrap(),
            Rule::new("b", 1, "b", Busy).unwrap(),
        ],
        Vec::new(),
    );
    assert!(matches!(
        dup_priority,
        Err(ClassifyError::DuplicatePriority(1))
    ));

    let dup_id = Manifest::new(
        AgentKind::Claude,
        15,
        None,
        vec![
            Rule::new("a", 1, "a", Busy).unwrap(),
            Rule::new("a", 2, "b", Busy).unwrap(),
        ],
        Vec::new(),
    );
    assert!(matches!(dup_id, Err(ClassifyError::DuplicateId(_))));
}

#[test]
fn bad_pattern_is_an_error_not_a_panic() {
    assert!(matches!(
        Rule::new("x", 1, "(", Busy),
        Err(ClassifyError::BadPattern { .. })
    ));
}

#[test]
fn every_builtin_manifest_builds() {
    for kind in [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::OpenCode,
        AgentKind::Pi,
    ] {
        let m = Manifest::builtin(kind).unwrap();
        assert_eq!(m.agent(), kind);
        assert!(!m.screen_rules().is_empty());
    }
}
