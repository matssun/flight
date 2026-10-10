// SPDX-License-Identifier: MIT

use super::*;
use crate::presentation::{side_by_side, PresentationOutcome};
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
    presentation_end: TerminalEnd,
    presentation_layout: Option<Layout>,
    sessions: RefCell<Vec<SurfaceChoice>>,
    presentations: RefCell<Vec<Layout>>,
}

impl Fake {
    fn new(session_end: TerminalEnd) -> Self {
        Self {
            session_end,
            session_undelivered: 0,
            presentation_end: TerminalEnd::UserLeft,
            presentation_layout: None,
            sessions: RefCell::default(),
            presentations: RefCell::default(),
        }
    }
}

impl Terminals for Fake {
    fn session(&self, request: SessionRequest) -> SessionOutcome {
        self.sessions.borrow_mut().push(request.shown.choice);
        SessionOutcome {
            end: self.session_end.clone(),
            shown: Some(request.shown.choice),
            undelivered: self.session_undelivered,
        }
    }

    fn presentation(&self, _workspace: WorkspaceKey, layout: Layout) -> PresentationOutcome {
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
