// SPDX-License-Identifier: MIT

use flight_classify::{
    classify_screen, classify_title, refine_title_with_screen, AgentKind, Manifest, Observation,
};
use flight_state::AgentState::{Busy, Permit, Question};

fn screen(kind: AgentKind, lines: &[&str]) -> Option<(flight_state::AgentState, String)> {
    let m = Manifest::builtin(kind).unwrap();
    let obs = Observation {
        screen_lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        title: String::new(),
    };
    classify_screen(&obs, &m).map(|c| (c.state, c.rule_id.to_string()))
}

fn title(kind: AgentKind, t: &str) -> Option<(flight_state::AgentState, String)> {
    let m = Manifest::builtin(kind).unwrap();
    let obs = Observation {
        screen_lines: Vec::new(),
        title: t.to_owned(),
    };
    classify_title(&obs, &m).map(|c| (c.state, c.rule_id.to_string()))
}

#[test]
fn opencode_frames() {
    let oc = AgentKind::OpenCode;
    assert_eq!(
        screen(
            oc,
            &[
                "│  Write src/index.ts",
                "",
                "△ Permission required",
                "",
                "  ↑↓ select · enter confirm · esc dismiss"
            ]
        ),
        Some((Permit, "permit.required".into()))
    );
    assert_eq!(
        screen(oc, &["  ↑↓ select · enter confirm · esc dismiss"]),
        Some((Permit, "permit.dismiss-confirm".into()))
    );
    assert_eq!(
        screen(oc, &["working · esc to interrupt"]),
        Some((Busy, "busy.esc-interrupt".into()))
    );
    assert_eq!(
        screen(oc, &["■■■■■■⬝⬝⬝⬝⬝⬝"]),
        Some((Busy, "busy.progress-bar".into()))
    );
    assert_eq!(screen(oc, &["■■■ partial"]), None);
    assert_eq!(
        screen(oc, &["△ Permission required", "working · esc to interrupt"]).map(|c| c.0),
        Some(Permit)
    );
    assert_eq!(
        screen(oc, &["$ ls", "src  README.md"]),
        None,
        "no prompt marker: never guesses Idle"
    );
}

#[test]
fn claude_titles() {
    assert_eq!(
        title(AgentKind::Claude, "⠂ fix flaky tests"),
        Some((Busy, "busy.title-spinner".into()))
    );
    assert_eq!(title(AgentKind::Claude, "✳ fix flaky tests"), None);
    assert_eq!(title(AgentKind::Claude, "x ⠂ not at the start"), None);
}

#[test]
fn codex_titles_blocked_outranks_working() {
    let cx = AgentKind::Codex;
    assert_eq!(
        title(cx, "Action Required"),
        Some((Permit, "permit.title-action-required".into()))
    );
    assert_eq!(
        title(cx, "⠇ refactor auth module"),
        Some((Busy, "busy.title-spinner".into()))
    );
    assert_eq!(title(cx, "- project"), None);
    assert_eq!(
        title(cx, "⠇ Action Required"),
        Some((Permit, "permit.title-action-required".into()))
    );
}

#[test]
fn managers_without_title_rules_never_title_match() {
    assert_eq!(title(AgentKind::OpenCode, "⠇ anything"), None);
    assert_eq!(title(AgentKind::Pi, "Action Required"), None);
}

#[test]
fn codex_question_screen_refines_the_shared_action_required_title() {
    let m = Manifest::builtin(AgentKind::Codex).unwrap();
    let obs = Observation {
        screen_lines: vec![
            "  tab to add notes | enter to submit answer".into(),
            "  esc to interrupt".into(),
        ],
        title: "Action Required".into(),
    };
    let t = classify_title(&obs, &m).unwrap();
    assert_eq!(t.state, Permit);
    let s = classify_screen(&obs, &m);
    let refined = refine_title_with_screen(AgentKind::Codex, t.clone(), s);
    assert_eq!(
        (refined.state, refined.rule_id.as_str()),
        (Question, "question.submit-answer")
    );
}

#[test]
fn refine_leaves_title_alone_for_permit_screens_and_other_agents() {
    let m = Manifest::builtin(AgentKind::Codex).unwrap();
    let obs = Observation {
        screen_lines: vec!["allow command?".into()],
        title: "Action Required".into(),
    };
    let t = classify_title(&obs, &m).unwrap();
    let kept = refine_title_with_screen(AgentKind::Codex, t.clone(), classify_screen(&obs, &m));
    assert_eq!(kept, t);
}

#[test]
fn pi_spinner_row_is_anchored_to_the_composer() {
    assert_eq!(
        screen(AgentKind::Pi, &["│ ⠹ │"]),
        Some((Busy, "busy.spinner-glyph".into()))
    );
    assert_eq!(screen(AgentKind::Pi, &["transcript with ⠹ inside"]), None);
}
