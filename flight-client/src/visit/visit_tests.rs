// SPDX-License-Identifier: MIT

use super::*;
use crate::presentation::{side_by_side, PresentationOutcome, WorkspaceSurfaces};
use crate::session::{Binding, SessionOutcome};
use crate::terminal::{SessionRequest, TerminalEnd};
use crate::ShownSurface;
use flight_present::Layout;
use flight_state::{HostId, PaneId, PaneRef, ServerId, SurfaceId, WorkspaceId};
use flight_ui::{SurfaceChoice, WorkspaceKey};
use std::cell::RefCell;
use std::path::PathBuf;

fn workspace() -> WorkspaceKey {
    WorkspaceKey {
        host: HostId::new("h"),
        workspace: WorkspaceId::new("w"),
    }
}

fn request(choice: SurfaceChoice) -> SessionRequest {
    SessionRequest {
        id: vec![1],
        shown: ShownSurface {
            workspace: workspace(),
            choice,
        },
        binding: Binding {
            pane: PaneRef {
                host: HostId::new("h"),
                server: ServerId::new("s"),
                pane: PaneId::new("%1"),
            },
            pid: 7,
        },
        typed_ahead: Vec::new(),
    }
}

/// Answers instead of taking over a terminal, and records what it was asked.
struct Fake {
    session_end: TerminalEnd,
    session_undelivered: usize,
    /// The surface on screen when the session ends, if not the one it began with.
    session_last_shown: Option<SurfaceChoice>,
    presentation_end: TerminalEnd,
    presentation_layout: Option<Layout>,
    sessions: RefCell<Vec<SurfaceChoice>>,
    presentations: RefCell<Vec<Layout>>,
    /// What each presentation was handed.
    handed: RefCell<Vec<Option<SurfaceChoice>>>,
    /// The surfaces the workspace has.
    surfaces: WorkspaceSurfaces,
}

impl Fake {
    fn new(session_end: TerminalEnd) -> Self {
        Self {
            session_end,
            session_undelivered: 0,
            session_last_shown: None,
            presentation_end: TerminalEnd::UserLeft,
            presentation_layout: None,
            sessions: RefCell::default(),
            presentations: RefCell::default(),
            handed: RefCell::default(),
            surfaces: WorkspaceSurfaces::standard(),
        }
    }
}

impl Terminals for Fake {
    /// The surface that was on screen.
    type Held = SurfaceChoice;

    fn session(&self, request: SessionRequest) -> (SessionOutcome, Option<SurfaceChoice>) {
        self.sessions.borrow_mut().push(request.shown.choice);
        let shown = self.session_last_shown.unwrap_or(request.shown.choice);
        let outcome = SessionOutcome {
            end: self.session_end.clone(),
            shown: Some(shown),
            undelivered: self.session_undelivered,
        };
        let held = (self.session_end == TerminalEnd::Presenting).then_some(shown);
        (outcome, held)
    }

    fn surfaces(&self, _workspace: &WorkspaceKey) -> WorkspaceSurfaces {
        self.surfaces.clone()
    }

    fn presentation(
        &self,
        _workspace: WorkspaceKey,
        _surfaces: WorkspaceSurfaces,
        layout: Layout,
        held: Option<SurfaceChoice>,
    ) -> PresentationOutcome {
        self.handed.borrow_mut().push(held);
        self.presentations.borrow_mut().push(layout.clone());
        PresentationOutcome {
            end: self.presentation_end.clone(),
            layout: self.presentation_layout.clone().unwrap_or(layout),
            undelivered: 0,
        }
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("flight-visit-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_session_that_ends_is_reported_and_the_same_workspace_is_selected_without_a_presentation() {
    let mut fake = Fake::new(TerminalEnd::UserLeft);
    fake.session_undelivered = 3;
    let back = visit(&fake, &dir("plain"), request(SurfaceChoice::Shell));
    assert_eq!(
        back.notice,
        "terminal: left the terminal (3 typed bytes were not delivered)"
    );
    assert_eq!(back.select, workspace());
    assert_eq!(*fake.sessions.borrow(), [SurfaceChoice::Shell]);
    assert!(fake.presentations.borrow().is_empty());
}

#[test]
fn asking_for_side_by_side_shows_both_with_the_keyboard_where_the_user_was_and_remembers_the_result(
) {
    let dir = dir("present");
    let fake = Fake::new(TerminalEnd::Presenting);
    let back = visit(&fake, &dir, request(SurfaceChoice::Shell));
    assert_eq!(back.notice, "terminal: left the terminal");
    assert_eq!(back.select, workspace());
    let shown = fake.presentations.borrow();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].focus(), &SurfaceId::new("shell"));
    assert_eq!(shown[0].surfaces().len(), 2);
    drop(shown);

    // What the user left is what the next visit starts from.
    let mut fake = Fake::new(TerminalEnd::Presenting);
    let (agent, shell) = (SurfaceId::new("agent"), SurfaceId::new("shell"));
    let tabbed = Layout::single(agent.clone())
        .add_tab(&agent, shell)
        .unwrap();
    assert_ne!(tabbed.root(), side_by_side(&agent).root());
    fake.presentation_layout = Some(tabbed.clone());
    visit(&fake, &dir, request(SurfaceChoice::Agent));
    let again = Fake::new(TerminalEnd::Presenting);
    visit(&again, &dir, request(SurfaceChoice::Agent));
    // (The keyboard goes to the surface the user opened, here the agent's tab.)
    let expected = tabbed.focus_on(&agent).unwrap();
    assert_eq!(again.presentations.borrow()[0].root(), expected.root());
}

#[test]
fn the_presentation_end_and_undelivered_input_are_what_is_reported() {
    let mut fake = Fake::new(TerminalEnd::Presenting);
    fake.presentation_end = TerminalEnd::Lost("gone".to_owned());
    let back = visit(&fake, &dir("end"), request(SurfaceChoice::Agent));
    assert_eq!(
        back.notice,
        "terminal: the terminal connection was lost: gone"
    );
}

#[test]
fn a_layout_store_that_cannot_be_used_is_said_and_the_default_arrangement_is_shown() {
    // A file in the place of the directory: nothing can be read or saved there.
    let not_a_dir = dir("broken").join("file");
    std::fs::write(&not_a_dir, "x").unwrap();
    let fake = Fake::new(TerminalEnd::Presenting);
    let back = visit(&fake, &not_a_dir, request(SurfaceChoice::Agent));
    assert!(
        back.notice
            .starts_with("terminal: left the terminal; layout not remembered: "),
        "{}",
        back.notice
    );
    assert_eq!(fake.presentations.borrow().len(), 1);
    assert_eq!(fake.presentations.borrow()[0].surfaces().len(), 2);
    assert_eq!(back.select, workspace());
}

#[test]
fn what_the_session_kept_goes_to_the_presentation_and_the_keyboard_is_where_the_user_last_was() {
    let mut fake = Fake::new(TerminalEnd::Presenting);
    // Opened on the agent, switched to the shell, then asked for both.
    fake.session_last_shown = Some(SurfaceChoice::Shell);
    visit(&fake, &dir("handed"), request(SurfaceChoice::Agent));
    assert_eq!(*fake.handed.borrow(), [Some(SurfaceChoice::Shell)]);
    assert_eq!(
        fake.presentations.borrow()[0].focus(),
        &SurfaceId::new("shell")
    );
    // A session that does not end in a presentation keeps nothing.
    let plain = Fake::new(TerminalEnd::UserLeft);
    visit(&plain, &dir("nothing-kept"), request(SurfaceChoice::Agent));
    assert!(plain.handed.borrow().is_empty());
}

#[test]
fn a_remembered_arrangement_keeps_a_third_surface_while_it_exists_and_the_agent_and_shell_always() {
    use crate::presentation::workspace_surfaces::fixtures::{pane, snapshot};
    let dir = dir("third");
    let with_third = {
        let snapshot = snapshot(vec![
            pane(
                "s-1",
                flight_ui::SurfaceKind::Agent(flight_classify::AgentKind::Claude),
                "agent",
                1,
            ),
            pane("s-2", flight_ui::SurfaceKind::Shell, "shell", 2),
            pane("s-3", flight_ui::SurfaceKind::Shell, "logs", 3),
        ]);
        WorkspaceSurfaces::of(&flight_ui::workspaces(&snapshot, "")[0])
    };
    let (agent, shell, logs) = (
        SurfaceId::new("agent"),
        SurfaceId::new("shell"),
        SurfaceId::new("s-3"),
    );
    let three = side_by_side(&agent)
        .split(
            &shell,
            flight_present::Axis::Down,
            logs.clone(),
            flight_present::Placement::After,
        )
        .unwrap();
    // The user leaves with three tiles.
    let mut first = Fake::new(TerminalEnd::Presenting);
    first.surfaces = with_third.clone();
    first.presentation_layout = Some(three.clone());
    visit(&first, &dir, request(SurfaceChoice::Agent));
    // Next time the workspace still has it: all three come back.
    let mut again = Fake::new(TerminalEnd::Presenting);
    again.surfaces = with_third;
    visit(&again, &dir, request(SurfaceChoice::Agent));
    assert_eq!(again.presentations.borrow()[0].surfaces().len(), 3);
    // When it is gone the arrangement is cut down to what is there; the agent and shell stay
    // even if they are down for a moment.
    let gone = Fake::new(TerminalEnd::Presenting);
    visit(&gone, &dir, request(SurfaceChoice::Agent));
    let shown = gone.presentations.borrow();
    assert_eq!(shown[0].surfaces().len(), 2);
    assert!(shown[0].contains(&agent) && shown[0].contains(&shell));
}
