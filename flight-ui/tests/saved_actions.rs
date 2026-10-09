// SPDX-License-Identifier: MIT

//! What the dashboard offers on a saved workspace: start it, look again, and the questions it
//! asks before it forgets, accepts or re-points one. It sends nothing it was not asked to.

mod support;

use flight_state::AgentState::Busy;
use flight_state::HostId;
use flight_ui::{
    render_to_string, Action, CreateFailure, Effect, HostHealth, InputMode, SavedActionKind,
    SavedActionRequest, SavedHealth, SavedOp, SavedPromptInput, SavedRoot, SavedView, UiSnapshot,
    ViewModel,
};
use support::*;

fn saved(name: &str, root: SavedRoot, imported: bool) -> SavedView {
    SavedView {
        host: HostId::new("dev1"),
        host_label: "dev1".to_owned(),
        host_health: HostHealth::Online,
        config_key: format!("c-{name}"),
        name: name.to_owned(),
        root: format!("/work/{name}"),
        health: if root == SavedRoot::Verified {
            SavedHealth::Stopped
        } else {
            SavedHealth::Blocked
        },
        root_state: root,
        detail: String::new(),
        running: None,
        imported,
    }
}

fn snapshot(saved: Vec<SavedView>, live: bool) -> UiSnapshot {
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

fn on(saved: SavedView) -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(snapshot(vec![saved], false));
    vm
}

fn request(effect: Effect) -> SavedActionRequest {
    match effect {
        Effect::SavedAction(r) => r,
        other => panic!("expected a saved action, got {other:?}"),
    }
}

#[test]
fn enter_asks_the_node_to_start_it_and_names_nothing_but_the_saved_identity() {
    let mut vm = on(saved("gone", SavedRoot::Verified, false));
    let r = request(vm.apply(Action::Switch));
    assert_eq!(r.action, SavedActionKind::Restore);
    assert_eq!((r.host.as_str(), r.config_key.as_str()), ("dev1", "c-gone"));
}

#[test]
fn r_looks_again_on_a_saved_workspace_and_refreshes_everywhere_else() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    assert_eq!(
        request(vm.apply(Action::Refresh)).action,
        SavedActionKind::Retry
    );
    let mut live = ViewModel::new();
    live.apply_snapshot(snapshot(vec![], true));
    assert_eq!(live.apply(Action::Refresh), Effect::Refresh);
}

#[test]
fn forgetting_asks_first_and_the_default_answer_is_no() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    assert_eq!(vm.apply(Action::SavedOp(SavedOp::Remove)), Effect::None);
    assert_eq!(vm.input_mode(), InputMode::SavedConfirm);
    // A stray Enter presses the focused button, which is Cancel.
    assert_eq!(
        vm.apply(Action::SavedPrompt(SavedPromptInput::Enter)),
        Effect::None
    );
    assert!(vm.saved_prompt().is_none());
    // y confirms.
    vm.apply(Action::SavedOp(SavedOp::Remove));
    let r = request(vm.apply(Action::SavedPrompt(SavedPromptInput::Yes)));
    assert_eq!(r.action, SavedActionKind::Remove);
    // Still waiting for the node: further input is ignored, no second request.
    assert_eq!(
        vm.apply(Action::SavedPrompt(SavedPromptInput::Yes)),
        Effect::None
    );
    vm.apply_saved_action(&r, Ok(()));
    assert!(vm.saved_prompt().is_none());
    assert!(vm
        .message()
        .unwrap()
        .contains("Nothing on dev1 was deleted"));
}

#[test]
fn a_refusal_keeps_the_question_open_with_the_reason() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    vm.apply(Action::SavedOp(SavedOp::Remove));
    let r = request(vm.apply(Action::SavedPrompt(SavedPromptInput::Yes)));
    vm.apply_saved_action(&r, Err(CreateFailure::Unreachable));
    let p = vm.saved_prompt().expect("still open");
    assert!(p.error().unwrap().contains("not connected"));
    assert!(!p.submitting(), "it can be tried again or cancelled");
    let screen = render_to_string(&vm, 120, 34);
    assert!(screen.contains("not connected"), "{screen}");
}

#[test]
fn changing_the_directory_is_typed_checked_and_sent_as_given() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    vm.apply(Action::SavedOp(SavedOp::ChangeRoot));
    assert_eq!(vm.input_mode(), InputMode::SavedInput);
    // Prefilled with the current directory; clear it and type another.
    for _ in 0..'/'.len_utf8() * "/work/gone".len() {
        vm.apply(Action::SavedPrompt(SavedPromptInput::Backspace));
    }
    // A relative path is refused in the prompt, nothing is sent.
    for c in "elsewhere".chars() {
        vm.apply(Action::SavedPrompt(SavedPromptInput::Char(c)));
    }
    assert_eq!(
        vm.apply(Action::SavedPrompt(SavedPromptInput::Enter)),
        Effect::None
    );
    assert!(vm
        .saved_prompt()
        .unwrap()
        .error()
        .unwrap()
        .contains("absolute"));
    for _ in 0.."elsewhere".len() {
        vm.apply(Action::SavedPrompt(SavedPromptInput::Backspace));
    }
    for c in "~/dev/nga".chars() {
        vm.apply(Action::SavedPrompt(SavedPromptInput::Char(c)));
    }
    let r = request(vm.apply(Action::SavedPrompt(SavedPromptInput::Enter)));
    assert_eq!(r.action, SavedActionKind::SetRoot("~/dev/nga".to_owned()));
    let screen = render_to_string(&vm, 120, 34);
    assert!(
        screen.contains("Change directory") && screen.contains("nothing is created"),
        "{screen}"
    );
}

#[test]
fn accepting_a_directory_is_only_offered_when_it_changed_and_asks() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    vm.apply(Action::SavedOp(SavedOp::AcceptRoot));
    assert!(vm.saved_prompt().is_none());
    assert!(vm.message().unwrap().contains("nothing to accept"));

    let mut vm = on(saved("moved", SavedRoot::Changed, false));
    vm.apply(Action::SavedOp(SavedOp::AcceptRoot));
    let screen = render_to_string(&vm, 120, 34);
    assert!(
        screen.contains("A different directory is at this path now"),
        "{screen}"
    );
    let r = request(vm.apply(Action::SavedPrompt(SavedPromptInput::Yes)));
    assert_eq!(r.action, SavedActionKind::AcceptRoot);
}

#[test]
fn trusting_is_only_offered_for_an_import_and_asks() {
    let mut vm = on(saved("mine", SavedRoot::Verified, false));
    vm.apply(Action::SavedOp(SavedOp::Trust));
    assert!(vm.saved_prompt().is_none());

    let mut vm = on(saved("shared", SavedRoot::Verified, true));
    vm.apply(Action::SavedOp(SavedOp::Trust));
    assert_eq!(
        request(vm.apply(Action::SavedPrompt(SavedPromptInput::Yes))).action,
        SavedActionKind::Trust
    );
}

#[test]
fn these_keys_do_nothing_on_a_live_workspace() {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(snapshot(vec![], true));
    for op in [
        SavedOp::ChangeRoot,
        SavedOp::Remove,
        SavedOp::AcceptRoot,
        SavedOp::Trust,
    ] {
        assert_eq!(vm.apply(Action::SavedOp(op)), Effect::None);
        assert!(vm.saved_prompt().is_none());
    }
}

#[test]
fn the_outcome_of_an_unprompted_action_is_one_line_with_the_reason_if_it_failed() {
    let mut vm = on(saved("gone", SavedRoot::Missing, false));
    let r = request(vm.apply(Action::Switch));
    vm.apply_saved_action(
        &r,
        Err(CreateFailure::NoSuchDirectory(
            "the directory is missing".into(),
        )),
    );
    assert!(vm.message().unwrap().contains("gone") && vm.message().unwrap().contains("missing"));
    vm.apply_saved_action(&r, Ok(()));
    assert!(vm.message().unwrap().contains("Started gone on dev1"));
}

#[test]
fn the_footer_offers_what_applies_to_the_selected_saved_workspace() {
    let plain = on(saved("gone", SavedRoot::Missing, false));
    let screen = render_to_string(&plain, 120, 30);
    assert!(
        screen.contains("Start") && screen.contains("Retry") && screen.contains("Forget"),
        "{screen}"
    );
    assert!(
        !screen.contains("Accept dir") && !screen.contains("Trust"),
        "{screen}"
    );
    let both = on(saved("odd", SavedRoot::Changed, true));
    let screen = render_to_string(&both, 140, 30);
    assert!(
        screen.contains("Accept dir") && screen.contains("Trust"),
        "{screen}"
    );
}

#[test]
fn no_prompt_speaks_backend_words() {
    let mut vm = on(saved("gone", SavedRoot::Changed, true));
    for op in [
        SavedOp::Remove,
        SavedOp::AcceptRoot,
        SavedOp::Trust,
        SavedOp::ChangeRoot,
    ] {
        vm.apply(Action::SavedOp(op));
        let screen = render_to_string(&vm, 120, 34).to_lowercase();
        for banned in ["tmux", "pane", "%1", "$1"] {
            assert!(!screen.contains(banned), "{banned} in\n{screen}");
        }
        vm.apply(Action::SavedPrompt(SavedPromptInput::Cancel));
    }
}
