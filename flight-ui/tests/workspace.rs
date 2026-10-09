// SPDX-License-Identifier: MIT

//! The dashboard in workspaces: one row per workspace, its agent and shell as surfaces of it,
//! attention derived from the agent, a shell offered (not assumed) when there is none, and
//! nothing on screen in the backend's words.

mod support;

use flight_state::AgentState::{Busy, Done, Idle, Permit, Question};
use flight_state::{HostId, SurfaceId, WorkspaceId};
use flight_ui::{
    workspaces, Action, Effect, InputMode, PromptInput, SurfaceChoice, SurfaceKind, UiSnapshot,
    ViewModel, WorkspaceKey,
};
use support::*;

fn vm_with(s: UiSnapshot) -> ViewModel {
    let mut vm = ViewModel::new();
    vm.apply_snapshot(s);
    vm
}

/// `nga` on dev1 with an agent in state `state`, and a shell if `with_shell`.
fn nga(state: flight_state::AgentState, with_shell: bool) -> UiSnapshot {
    let mut panes = vec![pane("dev1", "nga", "%1", state)];
    if with_shell {
        panes.push(shell_pane("dev1", "nga", "%2"));
    }
    snap(vec![online("dev1", panes)])
}

fn key(host: &str, session: &str) -> WorkspaceKey {
    WorkspaceKey {
        host: HostId::new(host),
        workspace: workspace_of(host, session),
    }
}

// --- the model -----------------------------------------------------------------------------

#[test]
fn an_agent_and_a_shell_of_one_workspace_are_one_workspace_with_two_surfaces() {
    let all = workspaces(&nga(Busy, true), "");
    assert_eq!(all.len(), 1);
    let w = &all[0];
    assert_eq!(w.name, "nga");
    assert_eq!(w.host, HostId::new("dev1"));
    assert_eq!(w.root, "/work");
    assert_eq!(w.surfaces.len(), 2);
    assert!(matches!(w.agent().unwrap().kind, SurfaceKind::Agent(_)));
    assert_eq!(w.shell().unwrap().kind, SurfaceKind::Shell);
    // Every surface belongs to exactly this workspace.
    assert!(w.surfaces.iter().all(|s| s.workspace == w.id));
    assert_ne!(w.agent().unwrap().id, w.shell().unwrap().id);
}

#[test]
fn a_workspace_without_a_shell_has_an_agent_alone() {
    let w = &workspaces(&nga(Busy, false), "")[0];
    assert!(w.shell().is_none() && w.agent().is_some());
}

#[test]
fn the_name_is_not_the_identity() {
    // Two workspaces with the same name on two hosts are two workspaces.
    let s = snap(vec![
        online("dev1", vec![pane("dev1", "nga", "%1", Busy)]),
        online("mac", vec![pane("mac", "nga", "%1", Busy)]),
    ]);
    let all = workspaces(&s, "");
    assert_eq!(all.len(), 2);
    assert_ne!(all[0].id, all[1].id);
    assert_ne!(all[0].key(), all[1].key());
    // Renaming the session keeps the workspace: the id, not the name, is what matches.
    let mut renamed = pane("dev1", "nga", "%1", Busy);
    renamed.session = "nga-renamed".into();
    let after = workspaces(&snap(vec![online("dev1", vec![renamed])]), "");
    assert_eq!(after[0].id, workspace_of("dev1", "nga"));
}

#[test]
fn ids_are_typed_and_compare_by_value_only() {
    let (w, s) = (WorkspaceId::new("w-1"), SurfaceId::new("w-1"));
    assert_eq!(w.as_str(), s.as_str());
    // (A `WorkspaceId` cannot be passed where a `SurfaceId` is wanted: that is the compiler's.)
    assert_eq!(WorkspaceId::new("w-1"), w);
    assert_ne!(WorkspaceId::new("w-2"), w);
}

#[test]
fn attention_comes_from_the_agent_and_only_from_the_agent() {
    // Agent asking, shell quiet: the workspace needs the user.
    let w = &workspaces(&nga(Question, true), "")[0];
    assert!(w.needs_attention());
    assert_eq!(w.state(), Question);
    // Agent busy: no attention, whatever the shell is doing.
    let mut s = nga(Busy, true);
    s.hosts[0].panes[1].state = Permit;
    let w = &workspaces(&s, "")[0];
    assert!(!w.needs_attention(), "a shell never asks for the user");
    assert_eq!(w.state(), Busy);
    // The surface states stay separate.
    assert_eq!(w.agent().unwrap().pane.state, Busy);
    assert_eq!(w.shell().unwrap().pane.state, Permit);
}

#[test]
fn workspaces_are_ordered_by_their_agents_urgency() {
    let s = snap(vec![online(
        "dev1",
        vec![
            pane("dev1", "calm", "%1", Idle),
            shell_pane("dev1", "calm", "%2"),
            pane("dev1", "asks", "%3", Question),
            pane("dev1", "works", "%4", Busy),
        ],
    )]);
    let names: Vec<_> = workspaces(&s, "").iter().map(|w| w.name.clone()).collect();
    assert_eq!(names, ["asks", "works", "calm"]);
}

#[test]
fn search_finds_a_workspace_by_name_host_or_what_it_has() {
    let s = nga(Busy, true);
    for (needle, hits) in [
        ("ng", 1),
        ("dev", 1),
        ("claude", 1),
        ("shell", 1),
        ("zzz", 0),
    ] {
        assert_eq!(workspaces(&s, needle).len(), hits, "{needle}");
    }
}

// --- selection and opening -----------------------------------------------------------------

#[test]
fn selection_follows_the_workspace_when_its_agent_goes_and_the_shell_remains() {
    let mut vm = vm_with(nga(Busy, true));
    assert_eq!(vm.selected_key(), Some(&key("dev1", "nga")));
    let agent_pane = vm.selected().unwrap();
    // The agent's window ends; the shell is what is left of the workspace.
    let mut left = nga(Busy, true);
    left.hosts[0].panes.remove(0);
    vm.apply_snapshot(left);
    assert_eq!(
        vm.selected_key(),
        Some(&key("dev1", "nga")),
        "same workspace"
    );
    assert_ne!(vm.selected().unwrap(), agent_pane, "now the shell's pane");
}

#[test]
fn enter_and_a_open_the_agent_and_s_the_shell() {
    let mut vm = vm_with(nga(Busy, true));
    let Effect::Switch(p) = vm.apply(Action::Switch) else {
        panic!("enter")
    };
    assert!(p.kind.is_agent());
    let Effect::Switch(p) = vm.apply(Action::Open(SurfaceChoice::Agent)) else {
        panic!("a")
    };
    assert!(p.kind.is_agent());
    let Effect::Switch(p) = vm.apply(Action::Open(SurfaceChoice::Shell)) else {
        panic!("s")
    };
    assert_eq!(p.kind, SurfaceKind::Shell);
    assert_eq!(p.workspace, workspace_of("dev1", "nga"));
}

#[test]
fn enter_on_a_workspace_without_an_agent_opens_its_shell() {
    let mut s = nga(Busy, true);
    s.hosts[0].panes.remove(0);
    let mut vm = vm_with(s);
    let Effect::Switch(p) = vm.apply(Action::Switch) else {
        panic!("enter")
    };
    assert_eq!(p.kind, SurfaceKind::Shell);
}

// --- the missing shell ---------------------------------------------------------------------

#[test]
fn s_without_a_shell_offers_to_make_one_and_does_not_make_it() {
    let mut vm = vm_with(nga(Busy, false));
    assert_eq!(vm.apply(Action::Open(SurfaceChoice::Shell)), Effect::None);
    let prompt = vm.prompt().expect("the offer");
    assert_eq!(prompt.name(), "nga");
    assert_eq!(prompt.root(), "/work");
    assert_eq!(prompt.host_label(), "dev1");
    assert_eq!(vm.input_mode(), InputMode::Prompt);
    let text = flight_ui::render_to_string(&vm, 100, 30);
    for want in [
        "Companion shell",
        "nga has no shell yet",
        "/work",
        "dev1",
        "Create",
        "Cancel",
    ] {
        assert!(text.contains(want), "{want}\n{text}");
    }
}

#[test]
fn confirming_asks_for_a_shell_by_workspace_alone_then_opens_it_when_it_exists() {
    let mut vm = vm_with(nga(Busy, false));
    vm.apply(Action::Open(SurfaceChoice::Shell));
    let Effect::CreateSurface(request) = vm.apply(Action::Prompt(PromptInput::Enter)) else {
        panic!("not a request")
    };
    // The request names the workspace and the kind: no host to pick, no directory to type.
    assert_eq!(request.workspace, workspace_of("dev1", "nga"));
    assert_eq!(request.kind, SurfaceChoice::Shell);
    assert!(vm.prompt().unwrap().submitting());

    // Created; the shell has not been published yet: nothing opens, the offer is closed.
    assert_eq!(vm.apply_surface_created(&request, Ok(())), Effect::None);
    assert!(vm.prompt().is_none());
    // The next snapshot has it: it opens by itself.
    let Effect::Switch(p) = vm.apply_snapshot(nga(Busy, true)) else {
        panic!("not opened")
    };
    assert_eq!(p.kind, SurfaceKind::Shell);
    // And only once.
    assert!(matches!(vm.apply_snapshot(nga(Busy, true)), Effect::None));
}

#[test]
fn cancelling_creates_nothing() {
    for cancel in [PromptInput::Cancel, PromptInput::Next] {
        let mut vm = vm_with(nga(Busy, false));
        vm.apply(Action::Open(SurfaceChoice::Shell));
        let effect = vm.apply(Action::Prompt(cancel));
        assert!(!matches!(effect, Effect::CreateSurface(_)));
    }
    let mut vm = vm_with(nga(Busy, false));
    vm.apply(Action::Open(SurfaceChoice::Shell));
    vm.apply(Action::Prompt(PromptInput::Next)); // focus Cancel
    assert_eq!(vm.apply(Action::Prompt(PromptInput::Enter)), Effect::None);
    assert!(vm.prompt().is_none());
}

#[test]
fn a_refusal_stays_in_the_prompt_in_plain_words() {
    use flight_ui::CreateFailure::{Other, UnknownWorkspace, Unreachable};
    for (failure, words) in [
        (Unreachable, "not connected"),
        (UnknownWorkspace, "gone from its host"),
        (
            Other("The shell could not start.".into()),
            "could not start",
        ),
    ] {
        let mut vm = vm_with(nga(Busy, false));
        vm.apply(Action::Open(SurfaceChoice::Shell));
        let Effect::CreateSurface(request) = vm.apply(Action::Prompt(PromptInput::Yes)) else {
            panic!("request")
        };
        vm.apply_surface_created(&request, Err(failure));
        let prompt = vm.prompt().expect("stays open");
        assert!(!prompt.submitting());
        assert!(
            prompt.error().unwrap().contains(words),
            "{:?}",
            prompt.error()
        );
        // It can be tried again.
        assert!(matches!(
            vm.apply(Action::Prompt(PromptInput::Yes)),
            Effect::CreateSurface(_)
        ));
    }
}

#[test]
fn a_shell_that_already_exists_by_the_time_is_just_opened() {
    let mut vm = vm_with(nga(Busy, false));
    vm.apply(Action::Open(SurfaceChoice::Shell));
    let Effect::CreateSurface(request) = vm.apply(Action::Prompt(PromptInput::Yes)) else {
        panic!("request")
    };
    vm.apply_snapshot(nga(Busy, true));
    let effect = vm.apply_surface_created(&request, Err(flight_ui::CreateFailure::AlreadyExists));
    assert!(matches!(effect, Effect::Switch(p) if p.kind == SurfaceKind::Shell));
}

// --- switching from inside a session --------------------------------------------------------

#[test]
fn resuming_opens_the_requested_surface_as_soon_as_it_is_listed() {
    let mut vm = ViewModel::new();
    vm.resume(key("dev1", "nga"), SurfaceChoice::Shell);
    let Effect::Switch(p) = vm.apply_snapshot(nga(Busy, true)) else {
        panic!("not opened")
    };
    assert_eq!(p.kind, SurfaceKind::Shell);
}

#[test]
fn resuming_to_a_shell_that_is_not_there_offers_to_make_it_instead_of_failing() {
    let mut vm = ViewModel::new();
    vm.resume(key("dev1", "nga"), SurfaceChoice::Shell);
    assert!(!matches!(
        vm.apply_snapshot(nga(Busy, false)),
        Effect::Switch(_)
    ));
    assert!(vm.prompt().is_some());
    assert_eq!(vm.selected_key(), Some(&key("dev1", "nga")));
}

#[test]
fn resuming_to_an_agent_that_is_gone_just_shows_the_dashboard() {
    let mut s = nga(Busy, true);
    s.hosts[0].panes.remove(0);
    let mut vm = ViewModel::new();
    vm.resume(key("dev1", "nga"), SurfaceChoice::Agent);
    assert!(!matches!(vm.apply_snapshot(s), Effect::Switch(_)));
    assert!(vm.prompt().is_none());
}

// --- what is drawn --------------------------------------------------------------------------

#[test]
fn the_selected_workspace_shows_its_agent_and_its_shell() {
    let vm = vm_with(nga(Busy, true));
    let text = flight_ui::render_to_string(&vm, 110, 30);
    assert!(text.contains("Workspaces"), "{text}");
    assert!(text.contains("nga"), "{text}");
    assert!(
        text.contains("a Agent") && text.contains("claude"),
        "{text}"
    );
    assert!(text.contains("s Shell") && text.contains("ready"), "{text}");
    // The host is secondary: on the row's right edge, not in the way of the name.
    let row = text.lines().find(|l| l.contains("▌ ⠋ nga")).unwrap();
    assert!(row.contains("dev1"), "{row}");
}

#[test]
fn without_a_shell_the_row_says_how_to_make_one() {
    let vm = vm_with(nga(Busy, false));
    let text = flight_ui::render_to_string(&vm, 110, 30);
    assert!(
        text.contains("s Shell") && text.contains("none yet"),
        "{text}"
    );
}

#[test]
fn an_agent_that_needs_the_user_puts_its_workspace_under_needs_you() {
    let s = snap(vec![online(
        "dev1",
        vec![
            pane("dev1", "calm", "%1", Idle),
            pane("dev1", "asks", "%2", Done),
            shell_pane("dev1", "asks", "%3"),
        ],
    )]);
    let text = flight_ui::render_to_string(&vm_with(s), 110, 30);
    let header = text.lines().position(|l| l.contains("NEEDS YOU")).unwrap();
    let asks = text.lines().position(|l| l.contains("● asks")).unwrap();
    let calm = text.lines().position(|l| l.contains("○ calm")).unwrap();
    assert!(header <= asks && asks < calm, "{text}");
    assert!(
        text.contains("1 ready"),
        "counted once, not per surface:\n{text}"
    );
}

#[test]
fn the_preview_says_where_the_workspace_is_and_whether_it_has_a_shell() {
    let with = flight_ui::render_to_string(&vm_with(nga(Busy, true)), 110, 30);
    assert!(with.contains("/work · shell ready"), "{with}");
    let without = flight_ui::render_to_string(&vm_with(nga(Busy, false)), 110, 30);
    assert!(without.contains("/work · no shell yet"), "{without}");
    assert!(with.contains("Ctrl-Space a agent"), "{with}");
}

#[test]
fn narrow_and_wide_stay_readable_with_surfaces() {
    let vm = vm_with(nga(Busy, true));
    for (w, h) in [(120, 30), (80, 24), (60, 20), (40, 16)] {
        let text = flight_ui::render_to_string(&vm, w, h);
        assert!(text.contains("nga"), "{w}x{h}\n{text}");
        assert!(text.contains("Shell"), "{w}x{h}\n{text}");
        for line in text.lines() {
            assert!(line.chars().count() <= usize::from(w), "{w}x{h}: {line}");
        }
    }
}

#[test]
fn no_screen_names_the_backend() {
    let mut vm = vm_with(nga(Busy, false));
    let mut screens = vec![flight_ui::render_to_string(&vm, 110, 30)];
    vm.apply(Action::Open(SurfaceChoice::Shell));
    screens.push(flight_ui::render_to_string(&vm, 110, 30));
    vm.apply(Action::Prompt(PromptInput::Cancel));
    vm.apply(Action::Help);
    screens.push(flight_ui::render_to_string(&vm, 110, 40));
    vm.apply(Action::CloseHelp);
    vm.apply(Action::NewSession);
    screens.push(flight_ui::render_to_string(&vm, 110, 30));
    for text in screens {
        let lower = text.to_lowercase();
        for banned in ["tmux", "pane", "window", "socket", "%1", "$"] {
            assert!(!lower.contains(banned), "{banned:?} in\n{text}");
        }
    }
}

#[test]
fn coming_back_from_a_session_puts_the_cursor_on_its_workspace() {
    let s = snap(vec![online(
        "dev1",
        vec![
            pane("dev1", "first", "%1", Question),
            pane("dev1", "second", "%2", Busy),
        ],
    )]);
    let mut vm = ViewModel::new();
    vm.point_at(key("dev1", "second"));
    // The first snapshot is the one from before the connection is up: empty.
    vm.apply_snapshot(snap(vec![]));
    vm.apply_snapshot(s.clone());
    assert_eq!(vm.selected_key(), Some(&key("dev1", "second")));
    // A workspace that is gone leaves the usual first choice.
    let mut vm = ViewModel::new();
    vm.point_at(key("dev1", "gone"));
    vm.apply_snapshot(s);
    assert_eq!(vm.selected_key(), Some(&key("dev1", "first")));
}

#[test]
fn a_quiet_shell_reads_ready_whatever_the_node_calls_its_quiet() {
    for quiet in [flight_state::AgentState::Shell, Idle] {
        let mut s = nga(Busy, true);
        s.hosts[0].panes[1].state = quiet;
        let text = flight_ui::render_to_string(&vm_with(s), 110, 30);
        let line = text.lines().find(|l| l.contains("s Shell")).unwrap();
        assert!(line.contains("ready"), "{quiet:?}: {line}");
    }
}
