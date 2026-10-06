// SPDX-License-Identifier: MIT

//! Selection is identity-based: it follows the pane, not the row, and the attention list is
//! a projection of the same panes the tree shows.

mod support;

use flight_state::AgentState::{Busy, Done, Idle, Permit, Question};
use flight_ui::{attention_panes, tree_panes, Action, Effect, Section, ViewModel};
use support::*;

fn fleet(states: [flight_state::AgentState; 4]) -> flight_ui::UiSnapshot {
    snap(vec![
        online(
            "mini-1",
            vec![
                pane("mini-1", "flight", "%1", states[0]),
                pane("mini-1", "nga", "%2", states[1]),
            ],
        ),
        online(
            "macbook",
            vec![
                pane("macbook", "hyperrag", "%1", states[2]),
                pane("macbook", "mcp-re", "%2", states[3]),
            ],
        ),
    ])
}

fn vm_with(s: flight_ui::UiSnapshot) -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(s);
    vm
}

#[test]
fn attention_is_a_projection_ordered_by_urgency_and_the_tree_is_state_independent() {
    let s = fleet([Done, Question, Permit, Busy]);
    let a: Vec<_> = attention_panes(&s)
        .iter()
        .map(|p| p.pane_ref.clone())
        .collect();
    assert_eq!(
        a,
        [
            pref("macbook", "%1"),
            pref("mini-1", "%2"),
            pref("mini-1", "%1")
        ],
        "Permit, Question, Done"
    );
    let t = |s: &flight_ui::UiSnapshot| {
        tree_panes(s)
            .iter()
            .map(|p| p.pane_ref.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        t(&s),
        t(&fleet([Busy, Idle, Done, Permit])),
        "tree order ignores state"
    );
    assert_eq!(
        t(&s).len(),
        4,
        "one record per pane: attention is a subset, not a copy"
    );
}

#[test]
fn first_snapshot_selects_the_most_urgent_pane() {
    let vm = vm_with(fleet([Busy, Question, Permit, Idle]));
    assert_eq!(
        (vm.selected().cloned(), vm.focus()),
        (Some(pref("macbook", "%1")), Section::Attention)
    );
}

#[test]
fn selection_stays_on_the_same_agent_when_states_reorder_the_attention_list() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down); // onto mini-1 %2 (Question)
    assert_eq!(vm.selected(), Some(&pref("mini-1", "%2")));
    // %1 on macbook (Permit) resolves, mini-1 %1 becomes Permit: order changes above and below.
    vm.apply_snapshot(fleet([Permit, Question, Idle, Busy]));
    assert_eq!(
        vm.selected(),
        Some(&pref("mini-1", "%2")),
        "cursor did not jump to another agent"
    );
    assert_eq!(vm.focus(), Section::Attention);
}

#[test]
fn selection_follows_its_pane_out_of_the_attention_set() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down);
    assert_eq!(vm.selected(), Some(&pref("mini-1", "%2")));
    let e = vm.apply_snapshot(fleet([Done, Busy, Permit, Busy])); // the question was answered
    assert_eq!(
        vm.selected(),
        Some(&pref("mini-1", "%2")),
        "still the same agent"
    );
    assert_eq!(
        vm.focus(),
        Section::Tree,
        "the cursor moved to where that pane now lives"
    );
    assert_eq!(
        e,
        Effect::None,
        "selection did not change, so no new preview request"
    );
}

#[test]
fn a_vanished_pane_falls_back_to_its_neighbour_not_to_nothing() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down);
    let gone = snap(vec![
        online("mini-1", vec![pane("mini-1", "flight", "%1", Done)]),
        online("macbook", vec![pane("macbook", "hyperrag", "%1", Permit)]),
    ]);
    let e = vm.apply_snapshot(gone);
    assert!(vm.selected().is_some());
    assert_eq!(e, Effect::Select(vm.selected().cloned()));
}

#[test]
fn an_empty_fleet_has_no_selection_and_actions_are_harmless() {
    let mut vm = vm_with(snap(vec![online("mini-1", vec![])]));
    assert_eq!(vm.selected(), None);
    for a in [
        Action::Up,
        Action::Down,
        Action::ToggleFocus,
        Action::Switch,
    ] {
        assert_eq!(vm.apply(a), Effect::None, "{a:?}");
    }
}

#[test]
fn up_and_down_clamp_and_report_the_new_selection() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    assert_eq!(vm.apply(Action::Up), Effect::None, "already at the top");
    assert_eq!(
        vm.apply(Action::Down),
        Effect::Select(Some(pref("mini-1", "%2")))
    );
    vm.apply(Action::Down);
    assert_eq!(
        vm.apply(Action::Down),
        Effect::None,
        "already at the bottom"
    );
}

#[test]
fn tab_moves_between_sections_keeping_the_pane_when_it_is_in_both() {
    let mut vm = vm_with(fleet([Permit, Busy, Busy, Busy]));
    let sel = vm.selected().cloned();
    assert_eq!(
        vm.apply(Action::ToggleFocus),
        Effect::None,
        "same pane, nothing to re-preview"
    );
    assert_eq!((vm.selected().cloned(), vm.focus()), (sel, Section::Tree));
    vm.apply(Action::Down);
    assert_eq!(vm.focus(), Section::Tree);
    assert_eq!(vm.selected(), Some(&pref("mini-1", "%2")));
    vm.apply(Action::ToggleFocus);
    assert_eq!(vm.focus(), Section::Attention);
    assert_eq!(
        vm.selected(),
        Some(&pref("mini-1", "%1")),
        "back to the first attention pane"
    );
}

#[test]
fn tab_does_nothing_when_the_other_section_is_empty() {
    let mut vm = vm_with(fleet([Busy, Busy, Idle, Idle]));
    assert_eq!(vm.focus(), Section::Tree);
    assert_eq!(vm.apply(Action::ToggleFocus), Effect::None);
    assert_eq!(vm.focus(), Section::Tree);
}

#[test]
fn enter_asks_to_switch_to_the_selected_pane() {
    let mut vm = vm_with(fleet([Permit, Busy, Busy, Busy]));
    assert_eq!(
        vm.apply(Action::Switch),
        Effect::Switch(pref("mini-1", "%1"))
    );
}

#[test]
fn quit_and_refresh_are_requests_not_actions_of_the_model() {
    let mut vm = vm_with(fleet([Permit, Busy, Busy, Busy]));
    assert_eq!(
        (vm.apply(Action::Quit), vm.apply(Action::Refresh)),
        (Effect::Quit, Effect::Refresh)
    );
}

#[test]
fn a_stale_preview_is_never_shown_for_another_pane() {
    use flight_ui::PanePreview;
    let mut vm = vm_with(fleet([Permit, Question, Busy, Busy]));
    vm.apply_preview(Some(PanePreview {
        pane: pref("mini-1", "%1"),
        content: Ok(vec!["a".into()]),
    }));
    assert!(vm.preview().is_some());
    vm.apply(Action::Down);
    assert!(
        vm.preview().is_none(),
        "preview belongs to the previous selection"
    );
}
