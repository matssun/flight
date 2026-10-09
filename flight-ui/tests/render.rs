// SPDX-License-Identifier: MIT

//! The real renderer, drawn to a test buffer. Tests check what the user can read and where,
//! not every character.

mod support;

use flight_state::AgentState::{Busy, Done, Down, Idle, Permit, Question, Shell};
use flight_ui::{
    layout_kind, render_to_string, session_at, Action, FilterInput, HostHealth, LayoutKind,
    PanePreview, ViewModel,
};
use ratatui::layout::Rect;
use support::*;

fn vm_with(s: flight_ui::UiSnapshot) -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(s);
    vm
}

fn fleet() -> flight_ui::UiSnapshot {
    snap(vec![
        online(
            "mini-1",
            vec![
                pane("mini-1", "nga", "%1", Question),
                pane("mini-1", "mcp-re", "%2", Permit),
                pane("mini-1", "flight", "%3", Busy),
            ],
        ),
        online("macbook", vec![pane("macbook", "hyperrag", "%1", Busy)]),
    ])
}

/// The line of `text` that contains `needle`.
fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in\n{text}"))
}

/// Only the list column of a wide screen (the first `cols` characters of every line).
fn list_column(text: &str, cols: usize) -> String {
    text.lines()
        .map(|l| l.chars().take(cols).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in\n{text}"))
}

#[test]
fn the_layout_follows_the_terminal_size() {
    assert_eq!(layout_kind(140, 40), LayoutKind::Wide);
    assert_eq!(layout_kind(100, 24), LayoutKind::Wide);
    assert_eq!(layout_kind(80, 30), LayoutKind::Stacked);
    assert_eq!(layout_kind(40, 24), LayoutKind::Stacked);
    assert_eq!(layout_kind(80, 12), LayoutKind::ListOnly);
}

#[test]
fn wide_puts_the_list_and_the_preview_side_by_side() {
    let text = render_to_string(&vm_with(fleet()), 120, 30);
    let line = line_with(&text, "Workspaces");
    assert!(line.contains("Preview"), "same row: {line}");
    assert!(line.find("Workspaces") < line.find("Preview"));
}

#[test]
fn narrow_stacks_the_preview_below_the_list() {
    let text = render_to_string(&vm_with(fleet()), 80, 30);
    assert!(
        row_of(&text, "Workspaces") < row_of(&text, "Preview"),
        "{text}"
    );
    assert!(!line_with(&text, "Workspaces").contains("Preview"));
}

#[test]
fn a_short_terminal_shows_the_list_alone() {
    let text = render_to_string(&vm_with(fleet()), 80, 12);
    assert!(
        text.contains("Workspaces") && !text.contains("Preview"),
        "{text}"
    );
    assert!(text.contains("mcp-re"));
}

#[test]
fn a_very_narrow_list_uses_two_line_cards_without_running_words_together() {
    let mut s = fleet();
    s.hosts.push(online(
        "mac-local",
        vec![pane("mac-local", "local-adr", "%1", Idle)],
    ));
    let text = render_to_string(&vm_with(s), 38, 60);
    let name = line_with(&text, "local-adr");
    assert!(
        !name.contains("mac-local"),
        "host is on its own line: {name}"
    );
    assert!(text.contains("idle · mac-local"), "{text}");
    assert!(!text.contains("mac-locallocal"));
}

#[test]
fn sessions_are_listed_urgency_first_with_readable_states() {
    let s = snap(vec![online(
        "h",
        vec![
            pane("h", "s-down", "%1", Down),
            pane("h", "s-shell", "%2", Shell),
            pane("h", "s-idle", "%3", Idle),
            pane("h", "s-busy", "%4", Busy),
            pane("h", "s-done", "%5", Done),
            pane("h", "s-question", "%6", Question),
            pane("h", "s-permit", "%7", Permit),
        ],
    )]);
    let text = list_column(&render_to_string(&vm_with(s), 100, 40), 46);
    let order = [
        "s-permit",
        "s-question",
        "s-done",
        "s-busy",
        "s-idle",
        "s-shell",
        "s-down",
    ];
    let rows: Vec<usize> = order.iter().map(|n| row_of(&text, n)).collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}\n{text}");
    for (name, word) in [
        ("s-permit", "waiting"),
        ("s-question", "asking"),
        ("s-done", "ready"),
        ("s-busy", "working"),
        ("s-idle", "idle"),
        ("s-shell", "shell"),
        ("s-down", "down"),
    ] {
        assert!(line_with(&text, name).contains(word), "{name}: {word}");
    }
    for glyph in ["⚠", "?", "●", "⠋", "○", "■"] {
        assert!(text.contains(glyph), "{glyph}");
    }
}

#[test]
fn groups_separate_what_needs_you_from_the_rest() {
    let text = list_column(&render_to_string(&vm_with(fleet()), 100, 30), 46);
    assert!(row_of(&text, "NEEDS YOU") < row_of(&text, "mcp-re"));
    assert!(row_of(&text, "mcp-re") < row_of(&text, "WORKING"));
    assert!(row_of(&text, "WORKING") < row_of(&text, "hyperrag"));
}

#[test]
fn the_header_summarises_attention_activity_and_hosts() {
    let text = render_to_string(&vm_with(fleet()), 100, 24);
    let header = text.lines().next().unwrap_or("");
    assert!(header.contains("Flight"));
    assert!(header.contains("2 need you"), "{header}");
    assert!(header.contains("2 working"), "{header}");
    assert!(header.contains("2 hosts"), "{header}");
}

#[test]
fn the_host_is_shown_on_the_row_but_never_replaces_the_session_name() {
    let text = render_to_string(&vm_with(fleet()), 100, 30);
    let row = line_with(&text, "hyperrag");
    assert!(row.contains("macbook"), "{row}");
    assert!(row.find("hyperrag") < row.find("macbook"));
}

#[test]
fn the_selected_session_has_a_marker_and_a_preview() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().expect("selection");
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Ok(vec!["old line".into(), "Do you want to proceed?".into()]),
    }));
    let text = render_to_string(&vm, 120, 30);
    assert!(
        line_with(&list_column(&text, 54), "mcp-re").contains('▌'),
        "the cursor is on the most urgent"
    );
    assert!(text.contains("Do you want to proceed?"), "{text}");
    assert!(
        text.contains("WAITING"),
        "the preview names the state: {text}"
    );
    assert!(
        text.contains("Enter opens it"),
        "and what Enter does: {text}"
    );
}

#[test]
fn the_preview_shows_the_bottom_of_a_long_screen_and_not_blank_tail_rows() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().expect("selection");
    let mut lines: Vec<String> = (0..100).map(|i| format!("row-{i}")).collect();
    lines.extend(std::iter::repeat_n(String::new(), 30));
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Ok(lines),
    }));
    let text = render_to_string(&vm, 120, 30);
    assert!(
        text.contains("row-99") && !text.contains("row-0 "),
        "{text}"
    );
}

#[test]
fn a_failed_capture_says_so_in_plain_words() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().expect("selection");
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Err("host unreachable".into()),
    }));
    assert!(render_to_string(&vm, 120, 30).contains("not available"));
}

#[test]
fn the_footer_shows_the_controls_and_the_legend() {
    let text = render_to_string(&vm_with(fleet()), 100, 24);
    for want in [
        "n New",
        "/ Search",
        "Enter Open",
        "? Help",
        "q Quit",
        "waiting",
        "working",
    ] {
        assert!(text.contains(want), "missing {want:?}\n{text}");
    }
}

#[test]
fn a_message_replaces_the_legend_and_loading_is_shown() {
    let mut vm = ViewModel::new();
    assert!(render_to_string(&vm, 100, 24).contains("connecting"));
    vm.apply_snapshot(fleet());
    vm.set_message(Some("cannot switch: no client".into()));
    assert!(render_to_string(&vm, 100, 24).contains("cannot switch: no client"));
}

#[test]
fn search_shows_the_query_and_only_matching_sessions() {
    let mut vm = vm_with(fleet());
    vm.apply(Action::Search);
    for c in "mac".chars() {
        vm.apply(Action::Filter(FilterInput::Char(c)));
    }
    let text = render_to_string(&vm, 100, 24);
    assert!(text.contains("/mac"), "{text}");
    assert!(
        text.contains("hyperrag") && !text.contains("mcp-re"),
        "{text}"
    );
    assert!(text.contains("Esc clear"));
}

#[test]
fn a_search_with_no_match_says_so_and_how_to_leave_it() {
    let mut vm = vm_with(fleet());
    vm.apply(Action::Search);
    for c in "zzz".chars() {
        vm.apply(Action::Filter(FilterInput::Char(c)));
    }
    let text = render_to_string(&vm, 100, 24);
    assert!(
        text.contains("No workspaces match") && text.contains("Esc clears the search"),
        "{text}"
    );
}

#[test]
fn with_no_sessions_a_new_user_is_told_what_to_do() {
    let text = render_to_string(&vm_with(snap(vec![online("dev1", vec![])])), 100, 24);
    assert!(text.contains("No Flight workspaces yet"), "{text}");
    assert!(text.contains("Press n to start a workspace"), "{text}");
    assert!(text.contains("connected hosts"));
}

#[test]
fn with_no_host_the_empty_state_says_that_instead() {
    let text = render_to_string(&vm_with(snap(vec![])), 100, 24);
    assert!(text.contains("No host is connected"), "{text}");
}

#[test]
fn host_failures_are_visible_and_do_not_hide_healthy_hosts() {
    let s = snap(vec![
        online("mini-1", vec![pane("mini-1", "flight", "%1", Busy)]),
        down(
            "mini-2",
            HostHealth::Unreachable("connection refused".into()),
        ),
        down("mini-3", HostHealth::AuthFailed("denied".into())),
        down("mini-4", HostHealth::NoTmux),
    ]);
    let text = render_to_string(&vm_with(s), 100, 30);
    for want in [
        "mini-2  unreachable",
        "mini-3  authentication failed",
        "mini-4  cannot run workspaces",
        "1 of 4 hosts online",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(text.contains("flight") && text.contains("working"));
}

#[test]
fn help_lists_navigation_state_meanings_and_actions() {
    let mut vm = vm_with(fleet());
    vm.apply(Action::Help);
    let text = render_to_string(&vm, 100, 40);
    for want in [
        "Help",
        "Moving around",
        "Enter",
        "Ctrl-Space q",
        "What the symbols mean",
        "needs your approval",
        "has a question for you",
        "finished, your move",
        "Actions",
        "new workspace",
    ] {
        assert!(text.contains(want), "missing {want:?}\n{text}");
    }
}

#[test]
fn nothing_a_user_reads_names_the_backend() {
    let mut s = fleet();
    s.hosts.push(down("a", HostHealth::NoServer));
    s.hosts.push(down("b", HostHealth::NoTmux));
    s.hosts.push(down("c", HostHealth::Failed("boom".into())));
    s.hosts.push(down("d", HostHealth::Disconnected));
    let mut vm = vm_with(s);
    let mut screens = vec![render_to_string(&vm, 120, 40)];
    screens.push(render_to_string(&vm, 80, 30));
    screens.push(render_to_string(&vm, 40, 24));
    vm.apply(Action::Help);
    screens.push(render_to_string(&vm, 100, 40));
    vm.apply(Action::CloseHelp);
    vm.apply(Action::NewSession);
    screens.push(render_to_string(&vm, 100, 40));
    screens.push(render_to_string(
        &vm_with(snap(vec![online("dev1", vec![])])),
        100,
        24,
    ));
    for text in screens {
        let lower = text.to_lowercase();
        for banned in ["tmux", "socket", "pane", "server", "window", "%1", "%2"] {
            assert!(!lower.contains(banned), "{banned:?} on screen:\n{text}");
        }
    }
}

#[test]
fn the_working_spinner_moves_and_nothing_else_does() {
    let mut vm = vm_with(fleet());
    let before = render_to_string(&vm, 100, 24);
    vm.tick();
    let after = render_to_string(&vm, 100, 24);
    assert_ne!(before, after, "the spinner advanced");
    let idle = vm_with(snap(vec![online("h", vec![pane("h", "x", "%1", Idle)])]));
    let mut idle2 = idle.clone();
    idle2.tick();
    assert_eq!(
        render_to_string(&idle, 100, 24),
        render_to_string(&idle2, 100, 24)
    );
}

#[test]
fn a_long_list_scrolls_to_keep_the_cursor_visible() {
    let panes: Vec<_> = (0..40)
        .map(|i| pane("mini-1", &format!("s{i:02}"), &format!("%{i}"), Busy))
        .collect();
    let mut vm = vm_with(snap(vec![online("mini-1", panes)]));
    for _ in 0..39 {
        vm.apply(Action::Down);
    }
    let text = render_to_string(&vm, 100, 12);
    assert!(
        text.lines().any(|l| l.contains('▌') && l.contains("s39")),
        "cursor row visible:\n{text}"
    );
}

#[test]
fn a_click_maps_to_the_session_drawn_under_it() {
    let vm = vm_with(fleet());
    let area = Rect::new(0, 0, 120, 30);
    let text = list_column(&render_to_string(&vm, 120, 30), 54);
    let row = u16::try_from(row_of(&text, "mcp-re")).expect("row");
    assert_eq!(session_at(&vm, area, 6, row), Some(pref("mini-1", "%2")));
    // A group heading, and the preview, are not sessions.
    let heading = u16::try_from(row_of(&text, "NEEDS YOU")).expect("row");
    assert_eq!(session_at(&vm, area, 6, heading), None);
    assert_eq!(session_at(&vm, area, 100, row), None);
}

#[test]
fn a_tiny_terminal_is_told_so_instead_of_garbled() {
    assert!(render_to_string(&vm_with(fleet()), 19, 4).contains("too small"));
}
