// SPDX-License-Identifier: MIT

//! Codex parity: cases ported from Fleet's `src/state/codex-detection.test.ts`
//! (MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md). Cases that exercise permit-key
//! resolution, discovery or fusion belong to other layers and are not ported here.

use flight_classify::{
    classify_screen, classify_title, refine_title_with_screen, AgentKind, Classification, Manifest,
    Observation, RuleId,
};
use flight_state::AgentState::{self, Busy, Idle, Permit, Question};

fn manifest() -> Manifest {
    Manifest::builtin(AgentKind::Codex).unwrap()
}

fn screen_of(lines: &[&str]) -> Option<Classification> {
    let obs = Observation {
        screen_lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        title: String::new(),
    };
    classify_screen(&obs, &manifest())
}

fn state_of(lines: &[&str]) -> Option<AgentState> {
    screen_of(lines).map(|c| c.state)
}

fn title_of(title: &str) -> Option<Classification> {
    let obs = Observation {
        screen_lines: Vec::new(),
        title: title.to_owned(),
    };
    classify_title(&obs, &manifest())
}

fn cls(state: AgentState, rule: &str) -> Classification {
    Classification {
        state,
        rule_id: RuleId::new(rule),
    }
}

const QUEUED_QUESTION: [&str; 7] = [
    "• Working (3m 46s • esc to interrupt)",
    "",
    "• Queued follow-up inputs",
    "  ? 1 question",
    "    shift + ← to answer",
    "",
    "› Ask Codex to do anything",
];

#[test]
fn permit_phrases() {
    for (lines, rule) in [
        (["Allow command?"], "permit.allow"),
        (
            ["Press Enter to confirm or Esc to cancel"],
            "permit.confirm",
        ),
        (["Overwrite file? [y/n]"], "permit.yn"),
        (["Do you want to apply this patch?"], "permit.do-you-want"),
    ] {
        assert_eq!(screen_of(&lines), Some(cls(Permit, rule)), "{lines:?}");
    }
}

#[test]
fn bare_prompt_marker_is_idle_and_unrecognized_is_none() {
    assert_eq!(
        screen_of(&["patch applied.", "", "❯"]),
        Some(cls(Idle, "idle.prompt"))
    );
    assert_eq!(screen_of(&["$ ls", "README.md", "src"]), None);
}

#[test]
fn first_match_wins_on_a_line_matching_several_rules() {
    assert_eq!(
        screen_of(&["Do you want to allow command? [y/n]"]),
        Some(cls(Permit, "permit.allow"))
    );
}

#[test]
fn styled_live_capture_reads_question_and_input_is_not_mutated() {
    let lines = [
        "\x1b[1m•\x1b[0m Working (13m • esc to interrupt)",
        "\x1b[2m• \x1b[0mQueued follow-up inputs",
        "\x1b[2m  ? \x1b[0;1m\x1b[38;5;6m1 question\x1b[0m",
        "\x1b[2m    shift + ← to answer\x1b[0m",
        "\x1b[1m›\x1b[0m Ask Codex to do anything",
    ];
    let obs = Observation {
        screen_lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        title: String::new(),
    };
    let before = obs.clone();
    assert_eq!(
        classify_screen(&obs, &manifest()).map(|c| c.state),
        Some(Question)
    );
    assert_eq!(obs, before);
}

#[test]
fn queued_question_wins_over_concurrent_work_and_question_prose() {
    let mut with_prose = vec!["Do you want to continue?"];
    with_prose.extend(QUEUED_QUESTION);
    assert_eq!(
        screen_of(&with_prose),
        Some(cls(Question, "question.queued-follow-up"))
    );

    let two: Vec<String> = QUEUED_QUESTION
        .iter()
        .map(|s| s.replace("1 question", "2 questions"))
        .collect();
    let two: Vec<&str> = two.iter().map(String::as_str).collect();
    assert_eq!(state_of(&two), Some(Question));
}

#[test]
fn active_answer_footers_read_question() {
    for footer in [
        "enter to submit all | tab to add notes | esc to interrupt",
        "enter to submit answer | ←/→ to navigate questions | esc to interrupt",
        "enter to submit all | tab to add notes |\n  esc to interrupt",
    ] {
        let lines: Vec<&str> = footer.split('\n').collect();
        assert_eq!(state_of(&lines), Some(Question), "{footer:?}");
    }
}

#[test]
fn async_shortcuts_distinguish_questions_from_the_action_required_title() {
    for plus in [" + ", "+"] {
        for main in [format!("alt{plus}↓"), format!("shift{plus}→")] {
            for next in [
                String::new(),
                format!("   shift{plus}← next question"),
                format!("\n    shift{plus}← next question"),
            ] {
                let footer =
                    format!("  enter submit   ctrl{plus}] skip   {main} main prompt{next}");
                let mut lines = vec![
                    "• Working (7m 38s • esc to interrupt)",
                    "• Queued follow-up inputs",
                    "  1 of 2",
                    "When S gives the permission error, are you opening a question?",
                    "› 1. Opening an existing question",
                    "  2. Sending a new message",
                ];
                lines.extend(footer.split('\n'));
                let screen = screen_of(&lines);
                assert_eq!(
                    screen,
                    Some(cls(Question, "question.async-answer")),
                    "{footer:?}"
                );
                let title = title_of("[ ! ] Action Required | Codex").unwrap();
                assert_eq!(
                    refine_title_with_screen(AgentKind::Codex, title, screen.clone()),
                    screen.unwrap()
                );
            }
        }
        let collapsed: Vec<String> = QUEUED_QUESTION
            .iter()
            .map(|l| l.replace(" + ", plus))
            .collect();
        let collapsed: Vec<&str> = collapsed.iter().map(String::as_str).collect();
        assert_eq!(
            screen_of(&collapsed),
            Some(cls(Question, "question.queued-follow-up"))
        );
    }
}

#[test]
fn quoted_prose_missing_structure_and_old_history_are_not_questions() {
    let old_history: Vec<&str> = QUEUED_QUESTION
        .iter()
        .copied()
        .chain(std::iter::repeat_n("ordinary output", 16))
        .collect();
    let cases: Vec<Vec<&str>> = vec![
        vec!["The shortcut is shift + ← to answer."],
        vec!["  ? 1 question", "    shift + ← to answer"],
        vec![
            "> • Queued follow-up inputs",
            ">   ? 1 question",
            ">     shift + ← to answer",
        ],
        old_history,
        vec!["I saw enter to submit all | tab to add notes | esc to interrupt"],
        vec!["The shortcut is shift+← to answer."],
        vec!["> enter submit   ctrl+] skip   alt+↓ main prompt"],
        vec!["enter submit   ctrl+] skip"],
        vec!["enter submit   ctrl+] skip   alt+↓ main prompt is the footer I saw"],
        vec!["> enter submit   ctrl+] skip   shift+→ main prompt"],
        vec!["enter submit   ctrl+] skip   shift+→ main prompt is the footer I saw"],
        vec!["enter submit   ctrl+] skip   shift+↓ main prompt"],
    ];
    for lines in cases {
        assert_ne!(state_of(&lines), Some(Question), "{lines:?}");
    }
}

#[test]
fn action_required_title_is_refined_by_a_queued_question_screen() {
    let screen = screen_of(&QUEUED_QUESTION).unwrap();
    let title = title_of("Action Required | Codex").unwrap();
    assert_eq!(
        refine_title_with_screen(AgentKind::Codex, title, Some(screen.clone())),
        screen
    );
}

#[test]
fn working_titles_real_permissions_and_other_agents_keep_their_precedence() {
    let screen = screen_of(&QUEUED_QUESTION);
    let working = cls(Busy, "busy.title-spinner");
    assert_eq!(
        refine_title_with_screen(AgentKind::Codex, working.clone(), screen.clone()),
        working
    );

    let permit = title_of("Action Required").unwrap();
    assert_eq!(
        refine_title_with_screen(AgentKind::Claude, permit.clone(), screen),
        permit
    );

    let real_permit_screen = screen_of(&["Press Enter to confirm or Esc to cancel"]);
    assert_eq!(
        refine_title_with_screen(AgentKind::Codex, permit.clone(), real_permit_screen),
        permit
    );

    assert_eq!(
        state_of(&[
            "• Working (4m • esc to interrupt)",
            "› Ask Codex to do anything"
        ]),
        Some(Busy)
    );
}
