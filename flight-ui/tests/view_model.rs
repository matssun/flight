// SPDX-License-Identifier: MIT

//! Selection is identity-based: it follows the session, not the row. The list is one
//! urgency-ordered projection of the snapshot, narrowed by the search.

mod support;

use flight_state::AgentState::{Busy, Done, Idle, Permit, Question, Shell};
use flight_ui::{
    workspaces as sessions_of, Action, Effect, FilterInput, InputMode, Summary, Tier, ViewModel,
};
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

fn type_search(vm: &mut ViewModel, text: &str) {
    vm.apply(Action::Search);
    for c in text.chars() {
        vm.apply(Action::Filter(FilterInput::Char(c)));
    }
}

#[test]
fn sessions_are_listed_most_urgent_first_whatever_the_input_order() {
    let s = fleet([Idle, Busy, Done, Permit]);
    let order: Vec<_> = sessions_of(&s, "").iter().map(|w| w.state()).collect();
    assert_eq!(order, [Permit, Done, Busy, Idle]);
    let all = snap(vec![online(
        "h",
        vec![
            pane("h", "a", "%1", flight_state::AgentState::Down),
            pane("h", "b", "%2", Shell),
            pane("h", "c", "%3", Idle),
            pane("h", "d", "%4", Busy),
            pane("h", "e", "%5", Done),
            pane("h", "f", "%6", Question),
            pane("h", "g", "%7", Permit),
        ],
    )]);
    let names: Vec<_> = sessions_of(&all, "")
        .iter()
        .map(|w| w.name.clone())
        .collect();
    assert_eq!(names, ["g", "f", "e", "d", "c", "b", "a"]);
}

#[test]
fn tiers_group_what_needs_you_from_what_works_and_what_rests() {
    assert_eq!(Tier::of(Permit), Tier::NeedsYou);
    assert_eq!(Tier::of(Question), Tier::NeedsYou);
    assert_eq!(Tier::of(Done), Tier::NeedsYou);
    assert_eq!(Tier::of(Busy), Tier::Working);
    for s in [Idle, Shell, flight_state::AgentState::Down] {
        assert_eq!(Tier::of(s), Tier::Quiet);
    }
}

#[test]
fn first_snapshot_selects_the_most_urgent_session() {
    let vm = vm_with(fleet([Busy, Question, Permit, Idle]));
    assert_eq!(vm.selected(), Some(pref("macbook", "%1")));
}

#[test]
fn selection_stays_on_the_same_session_when_states_reorder_the_list() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down); // Permit, then Question
    assert_eq!(vm.selected(), Some(pref("mini-1", "%2")));
    vm.apply_snapshot(fleet([Permit, Question, Idle, Busy]));
    assert_eq!(
        vm.selected(),
        Some(pref("mini-1", "%2")),
        "the cursor did not jump to another session"
    );
}

#[test]
fn selection_follows_its_session_to_another_group_without_a_new_preview() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down);
    assert_eq!(vm.selected(), Some(pref("mini-1", "%2")));
    let e = vm.apply_snapshot(fleet([Done, Busy, Permit, Busy])); // the question was answered
    assert_eq!(vm.selected(), Some(pref("mini-1", "%2")));
    assert_eq!(e, Effect::None, "same session, so no new preview request");
}

#[test]
fn a_vanished_session_falls_back_to_its_neighbour_not_to_nothing() {
    let mut vm = vm_with(fleet([Done, Question, Permit, Busy]));
    vm.apply(Action::Down);
    let gone = snap(vec![
        online("mini-1", vec![pane("mini-1", "flight", "%1", Done)]),
        online("macbook", vec![pane("macbook", "hyperrag", "%1", Permit)]),
    ]);
    let e = vm.apply_snapshot(gone);
    assert!(vm.selected().is_some());
    assert_eq!(e, Effect::Select(vm.selected()));
}

#[test]
fn an_empty_fleet_has_no_selection_and_actions_are_harmless() {
    let mut vm = vm_with(snap(vec![online("mini-1", vec![])]));
    assert_eq!(vm.selected(), None);
    for a in [Action::Up, Action::Down, Action::Switch] {
        assert_eq!(vm.apply(a.clone()), Effect::None, "{a:?}");
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
    vm.apply(Action::Down);
    assert_eq!(
        vm.apply(Action::Down),
        Effect::None,
        "already at the bottom"
    );
}

#[test]
fn enter_asks_to_open_the_selected_session() {
    let mut vm = vm_with(fleet([Permit, Busy, Busy, Busy]));
    assert_eq!(
        vm.apply(Action::Switch),
        Effect::Switch(vm.snapshot().hosts[0].panes[0].clone())
    );
}

#[test]
fn clicking_a_listed_session_selects_it_and_anything_else_is_ignored() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    assert_eq!(
        vm.apply(Action::Select(pref("macbook", "%2"))),
        Effect::Select(Some(pref("macbook", "%2")))
    );
    assert_eq!(
        vm.apply(Action::Select(pref("nowhere", "%9"))),
        Effect::None
    );
    assert_eq!(vm.selected(), Some(pref("macbook", "%2")));
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
fn a_stale_preview_is_never_shown_for_another_session() {
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

#[test]
fn search_matches_session_host_and_agent_ignoring_case() {
    let s = fleet([Busy, Busy, Busy, Busy]);
    let names =
        |f: &str| -> Vec<String> { sessions_of(&s, f).iter().map(|w| w.name.clone()).collect() };
    assert_eq!(names("NGA"), ["nga"]);
    assert_eq!(names("macbook"), ["hyperrag", "mcp-re"]);
    assert_eq!(names("claude").len(), 4, "agent kind");
    assert!(names("zzz").is_empty());
    assert_eq!(names("  nga "), ["nga"], "surrounding spaces do not matter");
}

#[test]
fn typing_a_search_narrows_the_list_and_keeps_the_cursor_on_a_listed_session() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    assert_eq!(vm.input_mode(), InputMode::Dashboard);
    type_search(&mut vm, "mcp");
    assert_eq!(vm.input_mode(), InputMode::Search);
    assert_eq!(vm.filter(), "mcp");
    assert_eq!(vm.selected(), Some(pref("macbook", "%2")));
    assert_eq!(vm.listed().len(), 1);
    // Enter keeps the filter and returns keys to the dashboard.
    vm.apply(Action::Filter(FilterInput::Accept));
    assert_eq!(
        (vm.input_mode(), vm.filter()),
        (InputMode::Dashboard, "mcp")
    );
}

#[test]
fn escape_clears_the_search_first_and_only_then_quits() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    type_search(&mut vm, "nga");
    vm.apply(Action::Filter(FilterInput::Accept));
    assert_eq!(vm.apply(Action::Back), Effect::None);
    assert_eq!(vm.filter(), "");
    assert_eq!(vm.listed().len(), 4);
    assert_eq!(vm.apply(Action::Back), Effect::Quit);
}

#[test]
fn backspace_edits_the_search_and_an_unmatched_search_selects_nothing() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    type_search(&mut vm, "zz");
    assert_eq!(vm.selected(), None);
    vm.apply(Action::Filter(FilterInput::Backspace));
    vm.apply(Action::Filter(FilterInput::Backspace));
    assert!(vm.selected().is_some());
}

#[test]
fn help_opens_and_any_key_closes_it() {
    let mut vm = vm_with(fleet([Permit, Question, Done, Busy]));
    vm.apply(Action::Help);
    assert_eq!(vm.input_mode(), InputMode::Help);
    vm.apply(Action::CloseHelp);
    assert_eq!(vm.input_mode(), InputMode::Dashboard);
}

#[test]
fn the_summary_counts_every_session_and_host_regardless_of_the_search() {
    let mut s = fleet([Permit, Question, Done, Busy]);
    s.hosts.push(down(
        "dev1",
        flight_ui::HostHealth::Unreachable("refused".into()),
    ));
    let sum = Summary::of(&s);
    assert_eq!(
        (
            sum.need_you,
            sum.ready,
            sum.working,
            sum.idle,
            sum.workspaces()
        ),
        (2, 1, 1, 0, 4)
    );
    assert_eq!((sum.hosts, sum.hosts_up), (3, 2));
    let mut vm = vm_with(s);
    type_search(&mut vm, "nga");
    assert_eq!(Summary::of(vm.snapshot()).workspaces(), 4);
}
