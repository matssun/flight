// SPDX-License-Identifier: MIT

//! Claude screen rules; cases ported from Fleet's detection.test.ts.

use flight_classify::{classify_screen, AgentKind, Manifest, Observation};
use flight_state::AgentState::{self, Busy, Idle, Permit, Question};

fn run(lines: &[&str]) -> Option<(AgentState, String)> {
    let m = Manifest::builtin(AgentKind::Claude).unwrap();
    let obs = Observation {
        screen_lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        title: String::new(),
    };
    classify_screen(&obs, &m).map(|c| (c.state, c.rule_id.to_string()))
}

type Case<'a> = (&'a str, &'a [&'a str], Option<(AgentState, &'a str)>);

fn check(cases: &[Case]) {
    for (name, lines, want) in cases {
        let want = want.map(|(s, r)| (s, r.to_owned()));
        assert_eq!(run(lines), want, "case: {name}");
    }
}

#[test]
fn core_rules_reproduce_the_original_scraper() {
    check(&[
        (
            "permit [y/n]",
            &["Allow Edit?", "[y/n]"],
            Some((Permit, "permit.yn")),
        ),
        (
            "permit [Y/n]",
            &["Allow Read?", "[Y/n]"],
            Some((Permit, "permit.yn")),
        ),
        (
            "do-you-want",
            &["Do you want to proceed?"],
            Some((Permit, "permit.do-you-want")),
        ),
        (
            "enter-select",
            &["Enter to select · ↑/↓ to navigate · Esc to cancel"],
            Some((Question, "question.enter-select")),
        ),
        (
            "counter min",
            &["✻ Trapping Gollum… (1m 11s · ↓ 3.4k tokens)", "", "❯"],
            Some((Busy, "busy.token-counter-min")),
        ),
        (
            "counter sec",
            &["✢ Sharting… (8s · ↑ 240 tokens)", "", "❯"],
            Some((Busy, "busy.token-counter-sec")),
        ),
        (
            "esc interrupt",
            &["Running command…", "", "(esc to interrupt)", "❯"],
            Some((Busy, "busy.esc-interrupt")),
        ),
        (
            "idle marker",
            &["Done!", "", "❯"],
            Some((Idle, "idle.prompt")),
        ),
        (
            "bare spinner + prompt is idle",
            &["✶ Thinking…", "", "❯"],
            Some((Idle, "idle.prompt")),
        ),
        ("no match", &["$ ls", "file1.ts", "file2.ts"], None),
    ]);
}

#[test]
fn spinner_elapsed_variants() {
    let busy = Some((Busy, "busy.spinner-elapsed"));
    let idle = Some((Idle, "idle.prompt"));
    check(&[
        (
            "hook running",
            &[
                "✻ Choreographing… (running PreToolUse hook · 1m 44s · ↓ 6.8k tokens)",
                "",
                "❯",
            ],
            busy,
        ),
        (
            "still thinking",
            &[
                "✻ Pondering… (1m 11s · ↓ 3.7k tokens · still thinking with xhigh effort)",
                "",
                "❯",
            ],
            busy,
        ),
        (
            "past an hour",
            &["✻ Pondering… (1h 2m 3s · ↓ 120k tokens)", "", "❯"],
            busy,
        ),
        ("before first token", &["✻ Thinking… (3s)", "", "❯"], busy),
        (
            "outranks answered prompt",
            &[
                "Do you want to proceed?",
                "● Bash(ls)",
                "✻ Pondering… (running PreToolUse hook · 4s)",
                "",
                "❯",
            ],
            busy,
        ),
        (
            "finished turn summary",
            &["✻ Cooked for 2m 42s · done 2:46 PM", "", "❯"],
            idle,
        ),
        (
            "collapsed output",
            &[
                "● Bash(tail -n 3 run.log…)",
                "     … +8 lines (ctrl+o to expand)",
                "",
                "❯",
            ],
            idle,
        ),
        (
            "duration in prose",
            &["The build took a while (2m 3s) to finish.", "", "❯"],
            idle,
        ),
        (
            "separator without spaces",
            &["✻ Thinking… (3s·still thinking)", "", "❯"],
            idle,
        ),
        (
            "fields do not span lines",
            &["✻ Thinking… (running PreToolUse hook ·", "  3s)", "", "❯"],
            idle,
        ),
    ]);
}

#[test]
fn braille_glyph_range_is_inclusive_and_exact() {
    let glyph = Some((Busy, "busy.spinner-glyph"));
    let idle = Some((Idle, "idle.prompt"));
    check(&[
        ("glyph alone", &["⠹ Puzzling…", "", "❯"], glyph),
        ("U+2800", &["⠀ working", "❯"], glyph),
        ("U+28FF", &["⣿ working", "❯"], glyph),
        ("U+27FF is not a glyph", &["⟿ Thinking…", "❯"], idle),
        ("U+2900 is not a glyph", &["⤀ Thinking…", "❯"], idle),
        (
            "dingbat star is not braille",
            &["✶ Thinking…", "", "❯"],
            idle,
        ),
        (
            "ascii punctuation",
            &["done * [ok] a·b — no braille", "❯"],
            idle,
        ),
    ]);
}

#[test]
fn glyph_is_last_so_earlier_rules_win() {
    check(&[
        (
            "permit beats glyph",
            &["Allow Edit? [y/n]", "⠹ working", "❯"],
            Some((Permit, "permit.yn")),
        ),
        (
            "question beats glyph",
            &[
                "Enter to select · ↑/↓ to navigate · Esc to cancel",
                "⠹",
                "❯",
            ],
            Some((Question, "question.enter-select")),
        ),
        (
            "counter beats glyph",
            &["⠹ Trapping Gollum… (8s · ↑ 240 tokens)", "", "❯"],
            Some((Busy, "busy.token-counter-sec")),
        ),
    ]);
}

#[test]
fn live_indicators_outrank_a_lingering_answered_prompt() {
    check(&[
        (
            "answered prompt + counter",
            &[
                "│ Do you want to proceed?",
                "│ ❯ 1. Yes",
                "",
                "✽ Processing… (20m 29s · ↓ 45.4k tokens)",
                "",
                "❯",
            ],
            Some((Busy, "busy.token-counter-min")),
        ),
        (
            "lingering [y/n] + esc hint",
            &[
                "Allow Edit to /path/file.ts?",
                "[y/n]",
                "",
                "✻ Cranking… (esc to interrupt)",
                "❯",
            ],
            Some((Busy, "busy.esc-interrupt")),
        ),
        (
            "genuine dialog stays permit",
            &[
                "⏺ Bash(rm -rf node_modules && npm install)",
                "  ⎿  Running…",
                "",
                "│ Do you want to proceed?",
                "│ ❯ 1. Yes",
                "│   2. No, and tell Claude what to do differently (esc)",
            ],
            Some((Permit, "permit.do-you-want")),
        ),
        (
            "genuine question stays question",
            &[
                "1. Option A",
                "2. Option B",
                "Enter to select · ↑/↓ to navigate · Esc to cancel",
            ],
            Some((Question, "question.enter-select")),
        ),
        (
            "counter outranks lingering permission text",
            &[
                "│ waiting for permission",
                "│ ❯ 1. Yes",
                "",
                "✽ Processing… (20m 29s · ↓ 45.4k tokens)",
                "",
                "❯",
            ],
            Some((Busy, "busy.token-counter-min")),
        ),
    ]);
}

#[test]
fn field_tested_permit_phrases() {
    for (phrase, rule) in [
        ("waiting for permission", "permit.waiting-for-permission"),
        (
            "do you want to allow this connection?",
            "permit.allow-connection",
        ),
        ("tab to amend", "permit.tab-to-amend"),
        ("ctrl+e to explain", "permit.ctrl-e-explain"),
        ("run a dynamic workflow?", "permit.dynamic-workflow"),
    ] {
        assert_eq!(
            run(&[phrase, "❯"]),
            Some((Permit, rule.to_owned())),
            "{phrase}"
        );
    }
}

#[test]
fn ansi_colour_does_not_hide_a_prompt() {
    assert_eq!(
        run(&["\x1b[1mDo you \x1b[0mwant to proceed?"]),
        Some((Permit, "permit.do-you-want".to_owned()))
    );
}

#[test]
fn rules_match_only_the_bottom_window_but_the_marker_scans_everything() {
    let mut lines = vec!["Do you want to proceed?"];
    lines.extend(std::iter::repeat_n("scrollback", 20));
    assert_eq!(
        run(&lines),
        None,
        "prompt above the 15-line window is ignored"
    );

    let mut deep = vec!["❯"];
    deep.extend(std::iter::repeat_n("scrollback", 19));
    assert_eq!(run(&deep), Some((Idle, "idle.prompt".to_owned())));
}
