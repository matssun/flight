// SPDX-License-Identifier: MIT

//! Saved workspaces that are not running stay on the dashboard, with the host, the configured
//! root and the reason, whatever happened to their processes, directory or host (ADR-008).

mod support;

use flight_state::AgentState::Busy;
use flight_state::HostId;
use flight_ui::{
    render_to_string, unavailable, Action, Effect, HostHealth, SavedHealth, SavedRoot, SavedView,
    UiSnapshot, ViewModel,
};
use support::*;

fn saved(
    name: &str,
    health: SavedHealth,
    root: SavedRoot,
    detail: &str,
    host_health: HostHealth,
) -> SavedView {
    SavedView {
        host: HostId::new("dev1"),
        host_label: "dev1".to_owned(),
        host_health,
        config_key: format!("c-{name}"),
        name: name.to_owned(),
        root: format!("/work/{name}"),
        health,
        root_state: root,
        detail: detail.to_owned(),
        running: None,
        imported: false,
        resume: flight_ui::SavedResume::Unknown,
    }
}

fn with(saved: Vec<SavedView>, live: bool) -> UiSnapshot {
    let mut s = snap(vec![online(
        "dev1",
        if live {
            vec![pane("dev1", "nga", "%1", Busy)]
        } else {
            vec![]
        },
    )]);
    s.saved = saved;
    s
}

fn vm(s: UiSnapshot) -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(s);
    vm
}

fn missing() -> SavedView {
    saved(
        "gone",
        SavedHealth::Blocked,
        SavedRoot::Missing,
        "no such directory",
        HostHealth::Online,
    )
}

#[test]
fn an_unavailable_workspace_is_listed_with_host_root_and_reason_when_nothing_else_is() {
    let vm = vm(with(vec![missing()], false));
    let screen = render_to_string(&vm, 120, 30);
    for want in ["SAVED", "gone", "dev1", "directory missing"] {
        assert!(screen.contains(want), "{want:?} missing from\n{screen}");
    }
    assert!(!screen.contains("No Flight workspaces yet"), "{screen}");
    assert!(screen.contains("1 saved unavailable"), "{screen}");
}

#[test]
fn selecting_it_shows_the_root_the_failure_and_that_nothing_is_repaired() {
    let mut vm = vm(with(vec![missing()], false));
    assert_eq!(vm.selected_saved().map(|s| s.name), Some("gone".to_owned()));
    let screen = render_to_string(&vm, 140, 30);
    for want in [
        "/work/gone",
        "no such directory",
        "does not create or repair",
    ] {
        assert!(screen.contains(want), "{want:?} missing from\n{screen}");
    }
    // Opening a surface of it explains instead of doing something; Enter is the way to start it.
    let effect = vm.apply(Action::Open(flight_ui::SurfaceChoice::Agent));
    assert_eq!(effect, Effect::None);
    assert!(vm.message().unwrap().contains("not running"));
    assert!(matches!(vm.apply(Action::Switch), Effect::SavedAction(_)));
}

#[test]
fn each_failure_is_told_apart() {
    let cases = [
        (SavedRoot::Missing, "directory missing"),
        (SavedRoot::Unverified, "directory unverified"),
        (SavedRoot::PermissionDenied, "no permission"),
        (SavedRoot::NotADirectory, "not a directory"),
        (SavedRoot::Changed, "directory changed"),
    ];
    for (root, text) in cases {
        let v = saved("x", SavedHealth::Blocked, root, "", HostHealth::Online);
        let screen = render_to_string(&vm(with(vec![v], false)), 120, 30);
        assert!(screen.contains(text), "{text}\n{screen}");
    }
}

#[test]
fn an_unreachable_host_is_the_reason_whatever_the_last_known_root_was() {
    let v = saved(
        "away",
        SavedHealth::Blocked,
        SavedRoot::Missing,
        "",
        HostHealth::Disconnected,
    );
    let screen = render_to_string(&vm(with(vec![v], false)), 120, 30);
    assert!(screen.contains("host unreachable"), "{screen}");
    assert!(!screen.contains("directory missing"), "{screen}");
}

#[test]
fn a_stopped_workspace_with_a_good_root_is_listed_as_stopped_not_as_a_problem() {
    let v = saved(
        "idle",
        SavedHealth::Stopped,
        SavedRoot::Verified,
        "",
        HostHealth::Online,
    );
    let screen = render_to_string(&vm(with(vec![v], false)), 120, 30);
    assert!(screen.contains("stopped"), "{screen}");
    assert!(!screen.contains("saved unavailable"), "{screen}");
}

#[test]
fn a_running_saved_workspace_is_not_listed_twice() {
    let mut v = saved(
        "nga",
        SavedHealth::Running,
        SavedRoot::Verified,
        "",
        HostHealth::Online,
    );
    v.running = Some("w-1".to_owned());
    let s = with(vec![v], true);
    assert!(unavailable(&s, "").is_empty());
}

#[test]
fn blocked_ones_come_first_and_the_cursor_walks_live_then_saved() {
    let stopped = saved(
        "a-stopped",
        SavedHealth::Stopped,
        SavedRoot::Verified,
        "",
        HostHealth::Online,
    );
    let mut vm = vm(with(vec![stopped, missing()], true));
    let order: Vec<String> = unavailable(vm.snapshot(), "")
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(order, vec!["gone", "a-stopped"]);
    assert!(
        vm.selected_workspace().is_some(),
        "starts on the live workspace"
    );
    vm.apply(Action::Down);
    assert_eq!(vm.selected_saved().unwrap().name, "gone");
    vm.apply(Action::Down);
    assert_eq!(vm.selected_saved().unwrap().name, "a-stopped");
    vm.apply(Action::Down);
    assert_eq!(
        vm.selected_saved().unwrap().name,
        "a-stopped",
        "stops at the end"
    );
    vm.apply(Action::Up);
    vm.apply(Action::Up);
    assert!(vm.selected_workspace().is_some());
}

#[test]
fn the_selection_stays_on_a_saved_workspace_across_snapshots() {
    let mut vm = vm(with(vec![missing()], true));
    vm.apply(Action::Down);
    assert_eq!(vm.selected_saved().unwrap().name, "gone");
    vm.apply_snapshot(with(vec![missing()], true));
    assert_eq!(vm.selected_saved().unwrap().name, "gone");
}

#[test]
fn the_search_narrows_saved_workspaces_too() {
    let s = with(vec![missing()], true);
    assert_eq!(unavailable(&s, "gon").len(), 1);
    assert_eq!(unavailable(&s, "dev1").len(), 1);
    assert_eq!(unavailable(&s, "/work/gone").len(), 1);
    assert!(unavailable(&s, "zzz").is_empty());
}

#[test]
fn the_dashboard_still_speaks_no_backend_words_with_saved_workspaces_on_it() {
    let screen = render_to_string(&vm(with(vec![missing()], true)), 140, 30).to_lowercase();
    for banned in ["tmux", "pane", "%1", "$1"] {
        assert!(!screen.contains(banned), "{banned} on screen\n{screen}");
    }
}

#[test]
fn a_narrow_terminal_still_says_what_is_wrong() {
    let screen = render_to_string(&vm(with(vec![missing()], false)), 50, 24);
    assert!(
        screen.contains("gone") && screen.contains("directory missing"),
        "{screen}"
    );
}
