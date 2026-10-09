// SPDX-License-Identifier: MIT

//! The "New session" form: opened from the dashboard, driven by keys, validated before
//! anything is sent, and showing a node's refusal inside the form.

mod support;

use flight_state::AgentState::Busy;
use flight_ui::{
    render_to_string, Action, CreateFailure, Effect, Field, FormInput, HostHealth,
    NewSessionRequest, Program, ViewModel,
};
use support::*;

fn fleet() -> flight_ui::UiSnapshot {
    snap(vec![
        online("mac-local", vec![pane("mac-local", "old", "%1", Busy)]),
        online("dev1", vec![pane("dev1", "other", "%1", Busy)]),
        down("lost", HostHealth::Disconnected),
    ])
}

fn open() -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(fleet());
    assert_eq!(vm.apply(Action::NewSession), Effect::None);
    vm
}

fn input(vm: &mut ViewModel, i: FormInput) -> Effect {
    vm.apply(Action::Form(i))
}

fn type_text(vm: &mut ViewModel, text: &str) {
    for c in text.chars() {
        input(vm, FormInput::Char(c));
    }
}

fn clear(vm: &mut ViewModel, n: usize) {
    for _ in 0..n {
        input(vm, FormInput::Backspace);
    }
}

fn focus(vm: &ViewModel) -> Field {
    vm.form().expect("form open").focus()
}

fn error(vm: &ViewModel) -> Option<String> {
    vm.form().and_then(|f| f.error()).map(str::to_owned)
}

/// Fill name and directory and press Create; returns the effect.
fn submit(vm: &mut ViewModel, name: &str, dir: &str) -> Effect {
    while focus(vm) != Field::Name {
        input(vm, FormInput::Next);
    }
    clear(vm, 80);
    type_text(vm, name);
    input(vm, FormInput::Next);
    clear(vm, 80);
    type_text(vm, dir);
    while focus(vm) != Field::Create {
        input(vm, FormInput::Next);
    }
    input(vm, FormInput::Enter)
}

#[test]
fn n_opens_the_form_on_the_name_with_sensible_defaults() {
    let vm = open();
    let f = vm.form().expect("open");
    assert_eq!(f.focus(), Field::Name);
    assert_eq!(f.program(), Program::Claude);
    assert_eq!(f.dir(), "~");
    assert_eq!(f.name(), "");
}

#[test]
fn only_connected_nodes_are_offered_as_hosts() {
    let vm = open();
    let labels: Vec<&str> = vm
        .form()
        .unwrap()
        .hosts()
        .iter()
        .map(|h| h.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["mac-local", "dev1"],
        "the disconnected node is not offered"
    );
}

#[test]
fn the_form_starts_on_the_host_of_the_selected_pane() {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(fleet());
    // The first snapshot selects the first pane; move to the dev1 pane.
    while vm.selected().map(|p| p.host.as_str()) != Some("dev1") {
        vm.apply(Action::Down);
    }
    vm.apply(Action::NewSession);
    let f = vm.form().unwrap();
    assert_eq!(f.hosts()[f.host_index()].label, "dev1");
}

#[test]
fn tab_and_shift_tab_walk_the_fields_and_wrap() {
    let mut vm = open();
    let mut seen = vec![focus(&vm)];
    for _ in 0..6 {
        input(&mut vm, FormInput::Next);
        seen.push(focus(&vm));
    }
    assert_eq!(
        seen,
        [
            Field::Name,
            Field::Directory,
            Field::Start,
            Field::Create,
            Field::Cancel,
            Field::Host,
            Field::Name
        ]
    );
    input(&mut vm, FormInput::Prev);
    assert_eq!(focus(&vm), Field::Host);
    input(&mut vm, FormInput::Prev);
    assert_eq!(focus(&vm), Field::Cancel);
}

#[test]
fn enter_in_a_field_moves_on_and_does_not_submit() {
    let mut vm = open();
    type_text(&mut vm, "api");
    assert_eq!(input(&mut vm, FormInput::Enter), Effect::None);
    assert_eq!(focus(&vm), Field::Directory);
}

#[test]
fn typing_edits_the_focused_text_field_only() {
    let mut vm = open();
    type_text(&mut vm, "api");
    input(&mut vm, FormInput::Next);
    clear(&mut vm, 1);
    type_text(&mut vm, "/work");
    let f = vm.form().unwrap();
    assert_eq!((f.name(), f.dir()), ("api", "/work"));
    clear(&mut vm, 20);
    assert_eq!(vm.form().unwrap().dir(), "");
}

#[test]
fn host_and_program_are_chosen_with_the_arrow_keys() {
    let mut vm = open();
    input(&mut vm, FormInput::Prev); // Host (wraps back from Name)
    assert_eq!(focus(&vm), Field::Host);
    input(&mut vm, FormInput::Right);
    let f = vm.form().unwrap();
    assert_eq!(f.hosts()[f.host_index()].label, "dev1");
    input(&mut vm, FormInput::Right);
    let f = vm.form().unwrap();
    assert_eq!(f.hosts()[f.host_index()].label, "mac-local", "wraps");
    input(&mut vm, FormInput::Left);
    let f = vm.form().unwrap();
    assert_eq!(f.hosts()[f.host_index()].label, "dev1");

    while focus(&vm) != Field::Start {
        input(&mut vm, FormInput::Next);
    }
    input(&mut vm, FormInput::Right);
    assert_eq!(vm.form().unwrap().program(), Program::ClaudeSkipPermissions);
    input(&mut vm, FormInput::Right);
    assert_eq!(vm.form().unwrap().program(), Program::Shell);
    input(&mut vm, FormInput::Left);
    assert_eq!(vm.form().unwrap().program(), Program::ClaudeSkipPermissions);
    input(&mut vm, FormInput::Left);
    assert_eq!(vm.form().unwrap().program(), Program::Claude);
    input(&mut vm, FormInput::Char(' '));
    assert_eq!(vm.form().unwrap().program(), Program::ClaudeSkipPermissions);
}

#[test]
fn claude_without_permission_prompts_is_a_spelled_out_choice_that_is_sent_as_such() {
    let mut vm = open();
    while focus(&vm) != Field::Start {
        input(&mut vm, FormInput::Next);
    }
    assert_eq!(
        vm.form().unwrap().program(),
        Program::Claude,
        "never the default"
    );
    input(&mut vm, FormInput::Right);
    let text = render_to_string(&vm, 100, 30);
    assert!(text.contains("(•) Claude, no permission prompts"), "{text}");
    let effect = submit(&mut vm, "api", "/srv/work");
    let Effect::Create(req) = effect else {
        panic!("{effect:?} / {:?}", error(&vm));
    };
    assert_eq!(req.program, Program::ClaudeSkipPermissions);
}

#[test]
fn create_sends_exactly_the_chosen_host_name_directory_and_program() {
    let mut vm = open();
    input(&mut vm, FormInput::Prev);
    input(&mut vm, FormInput::Right); // dev1
    while focus(&vm) != Field::Start {
        input(&mut vm, FormInput::Next);
    }
    input(&mut vm, FormInput::Right); // Claude, no permission prompts
    input(&mut vm, FormInput::Right); // Shell
    let effect = submit(&mut vm, "api", "/srv/work");
    let Effect::Create(req) = effect else {
        panic!("{effect:?} / {:?}", error(&vm));
    };
    assert_eq!(req.host.as_str(), "dev1");
    assert_eq!(req.name, "api");
    assert_eq!(req.dir, "/srv/work");
    assert_eq!(req.program, Program::Shell);
    assert!(vm.form().unwrap().submitting());
}

#[test]
fn cancel_closes_the_form_and_sends_nothing() {
    for cancel in [
        vec![FormInput::Cancel],
        vec![FormInput::Prev, FormInput::Prev, FormInput::Enter],
    ] {
        let mut vm = open();
        type_text(&mut vm, "half-typed");
        let mut effects = Vec::new();
        for i in cancel {
            effects.push(input(&mut vm, i));
        }
        assert!(vm.form().is_none());
        assert!(effects.iter().all(|e| *e == Effect::None), "{effects:?}");
    }
}

#[test]
fn required_fields_are_validated_in_the_form_without_leaving_it() {
    let mut vm = open();
    let e = submit(&mut vm, "", "/work");
    assert_eq!(e, Effect::None);
    assert_eq!(focus(&vm), Field::Name);
    assert!(error(&vm).unwrap().contains("name"));

    let e = submit(&mut vm, "api", "");
    assert_eq!(e, Effect::None);
    assert_eq!(focus(&vm), Field::Directory);
    assert!(error(&vm).unwrap().contains("directory"));
    assert!(vm.form().is_some(), "validation never closes the form");
}

#[test]
fn an_invalid_session_name_is_refused_with_the_rule() {
    for bad in ["a b", "a.b", "a:b", "a;b", "-x", "$(x)"] {
        let mut vm = open();
        assert_eq!(submit(&mut vm, bad, "/work"), Effect::None, "{bad}");
        assert_eq!(focus(&vm), Field::Name);
        assert!(error(&vm).unwrap().contains("letters, digits"), "{bad}");
    }
}

#[test]
fn a_directory_must_be_absolute_or_home_relative() {
    let mut vm = open();
    assert_eq!(submit(&mut vm, "api", "work/x"), Effect::None);
    assert_eq!(focus(&vm), Field::Directory);
    assert!(matches!(
        submit(&mut vm, "api", "~/work"),
        Effect::Create(_)
    ));
}

#[test]
fn typing_is_bounded_by_the_protocol_limits() {
    let mut vm = open();
    type_text(&mut vm, &"a".repeat(200));
    assert_eq!(vm.form().unwrap().name().len(), 64);
}

#[test]
fn with_no_connected_node_the_form_says_so_and_cannot_submit() {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(snap(vec![down("lost", HostHealth::Disconnected)]));
    vm.apply(Action::NewSession);
    assert_eq!(submit(&mut vm, "api", "/work"), Effect::None);
    assert_eq!(focus(&vm), Field::Host);
    assert!(error(&vm).unwrap().contains("No node is connected"));
}

fn request(vm: &mut ViewModel) -> NewSessionRequest {
    let Effect::Create(req) = submit(vm, "api", "/work") else {
        panic!("{:?}", error(vm));
    };
    req
}

#[test]
fn each_typed_refusal_is_shown_in_the_form_on_the_field_to_fix() {
    let cases = [
        (CreateFailure::AlreadyExists, Field::Name, "already exists"),
        (
            CreateFailure::NoSuchDirectory("/work does not exist on this node".into()),
            Field::Directory,
            "does not exist",
        ),
        (
            CreateFailure::ProgramUnavailable("claude was not found on this node's PATH".into()),
            Field::Start,
            "claude was not found",
        ),
        (CreateFailure::Unreachable, Field::Host, "not connected"),
        (CreateFailure::Unsupported, Field::Create, "orchestrator"),
        (CreateFailure::Other("boom".into()), Field::Create, "boom"),
    ];
    for (failure, field, text) in cases {
        let mut vm = open();
        let req = request(&mut vm);
        vm.apply_created(&req, Err(failure.clone()));
        assert!(vm.form().is_some(), "{failure:?}: the form stays open");
        assert!(
            !vm.form().unwrap().submitting(),
            "{failure:?}: and can be edited"
        );
        assert_eq!(focus(&vm), field, "{failure:?}");
        assert!(
            error(&vm).unwrap().contains(text),
            "{failure:?}: {:?}",
            error(&vm)
        );
        // And the rendered form shows it.
        assert!(render_to_string(&vm, 100, 30).contains(text), "{failure:?}");
    }
}

#[test]
fn after_a_refusal_the_form_can_be_fixed_and_resubmitted() {
    let mut vm = open();
    let req = request(&mut vm);
    vm.apply_created(&req, Err(CreateFailure::AlreadyExists));
    assert_eq!(focus(&vm), Field::Name);
    clear(&mut vm, 10);
    type_text(&mut vm, "api2");
    assert!(error(&vm).is_none(), "editing clears the stale message");
    while focus(&vm) != Field::Create {
        input(&mut vm, FormInput::Next);
    }
    let Effect::Create(again) = input(&mut vm, FormInput::Enter) else {
        panic!("{:?}", error(&vm));
    };
    assert_eq!(again.name, "api2");
}

#[test]
fn input_is_ignored_while_a_request_is_in_flight() {
    let mut vm = open();
    let _ = request(&mut vm);
    assert_eq!(input(&mut vm, FormInput::Cancel), Effect::None);
    assert!(vm.form().is_some());
    assert!(render_to_string(&vm, 100, 30).contains("Creating"));
}

#[test]
fn success_returns_to_the_dashboard_and_selects_the_new_session_when_it_appears() {
    let mut vm = open();
    let req = request(&mut vm);
    vm.apply_created(&req, Ok(()));
    assert!(vm.form().is_none(), "back on the dashboard");
    assert!(vm.message().unwrap().contains("api"));
    let before = vm.selected().cloned();

    // Not in the snapshot yet: the cursor stays where it was.
    vm.apply_snapshot(fleet());
    assert_eq!(vm.selected(), before.as_ref());

    // Now the node reports it.
    let mut hosts = fleet();
    hosts.hosts[0]
        .panes
        .push(pane("mac-local", "api", "%9", Busy));
    let effect = vm.apply_snapshot(hosts);
    assert_eq!(vm.selected(), Some(&pref("mac-local", "%9")));
    assert_eq!(effect, Effect::Select(Some(pref("mac-local", "%9"))));
}

#[test]
fn a_created_session_on_another_host_with_the_same_name_is_not_selected() {
    let mut vm = open();
    let req = request(&mut vm); // mac-local / api
    vm.apply_created(&req, Ok(()));
    let mut hosts = fleet();
    hosts.hosts[1].panes.push(pane("dev1", "api", "%9", Busy));
    vm.apply_snapshot(hosts);
    assert_ne!(vm.selected(), Some(&pref("dev1", "%9")));
}

#[test]
fn moving_the_cursor_cancels_the_pending_selection() {
    let mut vm = open();
    let req = request(&mut vm);
    vm.apply_created(&req, Ok(()));
    vm.apply(Action::Down);
    let moved = vm.selected().cloned();
    let mut hosts = fleet();
    hosts.hosts[0]
        .panes
        .push(pane("mac-local", "api", "%9", Busy));
    vm.apply_snapshot(hosts);
    assert_eq!(vm.selected(), moved.as_ref(), "the user's choice wins");
}

#[test]
fn the_dashboard_says_how_to_open_the_form() {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(fleet());
    let text = render_to_string(&vm, 100, 24);
    assert!(text.contains("n New") && text.contains("? Help"), "{text}");
    let empty = {
        let mut vm = ViewModel::new();
        vm.apply_snapshot(snap(vec![online("mac-local", vec![])]));
        render_to_string(&vm, 100, 24)
    };
    assert!(
        empty.contains("No Flight sessions yet") && empty.contains("Press n to start"),
        "{empty}"
    );
}

#[test]
fn the_rendered_form_shows_fields_focus_and_buttons() {
    let mut vm = open();
    type_text(&mut vm, "api");
    let text = render_to_string(&vm, 100, 30);
    for want in [
        "New session",
        "Host",
        "mac-local",
        "▌ Name",
        "[api▏",
        "Directory",
        "[~",
        "(•) Claude",
        "( ) Shell",
        "Create",
        "Cancel",
        "Esc cancel",
    ] {
        assert!(text.contains(want), "missing {want:?} in\n{text}");
    }
    input(&mut vm, FormInput::Next);
    let text = render_to_string(&vm, 100, 30);
    assert!(text.contains("▌ Directory"), "{text}");
    assert!(!text.contains("▌ Name"));
}

#[test]
fn the_form_fits_a_small_terminal() {
    let vm = open();
    let text = render_to_string(&vm, 60, 16);
    assert!(text.contains("Create"), "{text}");
}
