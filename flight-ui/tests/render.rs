// SPDX-License-Identifier: MIT

//! The real renderer, drawn to a test buffer.

mod support;

use flight_state::AgentState::{Busy, Done, Idle, Permit, Question};
use flight_ui::{render_to_string, Action, HostHealth, PanePreview, ViewModel};
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

#[test]
fn shows_attention_then_hosts_with_states() {
    let text = render_to_string(&vm_with(fleet()), 100, 24);
    let attention = text.find("ATTENTION").expect("attention heading");
    let hosts = text.find("HOSTS").expect("hosts heading");
    assert!(attention < hosts);
    assert!(
        text.contains("mcp-re") && text.contains("waiting"),
        "{text}"
    );
    assert!(text.contains("nga") && text.contains("asking"));
    assert!(text.contains("hyperrag") && text.contains("working"));
    // Permit (mcp-re) is more urgent than Question (nga) and is listed first.
    assert!(text.find("mcp-re").unwrap() < text.find("nga").unwrap());
}

#[test]
fn the_cursor_marks_the_selected_row_in_the_focused_section() {
    let text = render_to_string(&vm_with(fleet()), 100, 24);
    let line = text
        .lines()
        .find(|l| l.contains("> "))
        .expect("a cursor row");
    assert!(
        line.contains("mcp-re"),
        "cursor starts on the most urgent pane: {line}"
    );
}

#[test]
fn host_failures_are_visible_and_do_not_hide_healthy_hosts() {
    let s = snap(vec![
        online("mini-1", vec![pane("mini-1", "flight", "%1", Busy)]),
        down(
            "mini-2",
            HostHealth::Unreachable("connection refused".into()),
        ),
        down("macbook", HostHealth::NoServer),
        down("mini-3", HostHealth::AuthFailed("denied".into())),
        down("mini-4", HostHealth::NoTmux),
    ]);
    let text = render_to_string(&vm_with(s), 100, 24);
    for want in [
        "● mini-1  online",
        "! mini-2  unreachable",
        "○ macbook  no sessions yet",
        "! mini-3  authentication failed",
        "! mini-4  session backend missing",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(
        text.contains("flight") && text.contains("working"),
        "healthy host still listed"
    );
}

#[test]
fn an_empty_attention_set_says_so() {
    let text = render_to_string(
        &vm_with(snap(vec![online(
            "mini-1",
            vec![pane("mini-1", "a", "%1", Idle)],
        )])),
        100,
        24,
    );
    assert!(text.contains("nothing needs you"));
}

#[test]
fn preview_shows_the_selected_pane_s_screen_and_provenance() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().cloned().unwrap();
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Ok(vec!["old line".into(), "Do you want to proceed?".into()]),
    }));
    let text = render_to_string(&vm, 100, 24);
    assert!(text.contains("Do you want to proceed?"), "{text}");
    assert!(
        text.contains("mini-1 / mcp-re"),
        "preview title names the pane: {text}"
    );
}

#[test]
fn preview_shows_the_bottom_of_a_long_screen() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().cloned().unwrap();
    let lines: Vec<String> = (0..100).map(|i| format!("row-{i}")).collect();
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Ok(lines),
    }));
    let text = render_to_string(&vm, 100, 24);
    assert!(
        text.contains("row-99") && !text.contains("row-0 "),
        "{text}"
    );
}

#[test]
fn a_failed_capture_is_reported_in_the_preview() {
    let mut vm = vm_with(fleet());
    let sel = vm.selected().cloned().unwrap();
    vm.apply_preview(Some(PanePreview {
        pane: sel,
        content: Err("host unreachable".into()),
    }));
    assert!(render_to_string(&vm, 100, 24).contains("cannot capture: host unreachable"));
}

#[test]
fn narrow_terminals_drop_the_preview_but_keep_the_list() {
    let text = render_to_string(&vm_with(fleet()), 60, 24);
    assert!(text.contains("ATTENTION") && !text.contains("loading"));
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
        text.lines().any(|l| l.contains("> ") && l.contains("s39")),
        "cursor row visible:\n{text}"
    );
}

#[test]
fn status_line_shows_keys_then_messages_then_loading() {
    let mut vm = ViewModel::new();
    assert!(render_to_string(&vm, 100, 10).contains("loading"));
    vm.apply_snapshot(fleet());
    assert!(render_to_string(&vm, 100, 10).contains("Enter switch"));
    vm.set_message(Some("cannot switch: no client".into()));
    assert!(render_to_string(&vm, 100, 10).contains("cannot switch: no client"));
}

#[test]
fn ready_agents_read_as_ready() {
    let text = render_to_string(
        &vm_with(snap(vec![online(
            "mini-1",
            vec![pane("mini-1", "a", "%1", Done)],
        )])),
        100,
        12,
    );
    assert!(text.contains("ready"));
}
