// SPDX-License-Identifier: MIT

use super::returning::Returning;
use super::terminals::Terminals;
use crate::presentation::{remember, side_by_side, starting_layout, surface_choice, surface_id};
use crate::session::SessionOutcome;
use crate::terminal::{SessionRequest, TerminalEnd};
use crate::LayoutStore;
use std::path::Path;

/// Give the user's terminal to the workspace in `request`, and report one outcome when it is
/// handed back. A surface is shown full screen; if the user asks for the surfaces side by side
/// that is shown next, from the arrangement they left last time (kept in `layout_dir`), and the
/// arrangement they leave is remembered. A layout that cannot be read or saved is said in the
/// notice, and the arrangement is simply not remembered.
pub fn visit<T: Terminals>(terminals: &T, layout_dir: &Path, request: SessionRequest) -> Returning {
    let workspace = request.shown.workspace.clone();
    let choice = request.shown.choice;
    let (mut outcome, held) = terminals.session(request);
    let mut problems = Vec::new();
    if outcome.end == TerminalEnd::Presenting {
        // The surface the user was last looking at has the keyboard.
        let focus = outcome.shown.unwrap_or(choice);
        outcome = present(
            terminals,
            layout_dir,
            &workspace,
            focus,
            held,
            &mut problems,
        );
    }
    let mut notice = format!("terminal: {}", outcome.end);
    if outcome.undelivered > 0 {
        notice.push_str(&format!(
            " ({} typed bytes were not delivered)",
            outcome.undelivered
        ));
    }
    for problem in problems {
        notice.push_str("; ");
        notice.push_str(&problem);
    }
    Returning {
        notice,
        select: workspace,
    }
}

fn present<T: Terminals>(
    terminals: &T,
    layout_dir: &Path,
    workspace: &flight_ui::WorkspaceKey,
    choice: flight_ui::SurfaceChoice,
    held: Option<T::Held>,
    problems: &mut Vec<String>,
) -> SessionOutcome {
    let focus = surface_id(choice);
    let mut store = LayoutStore::open(layout_dir);
    let layout = match &store {
        Ok(store) => starting_layout(Some(store), workspace, &focus, |s| {
            surface_choice(s).is_some()
        }),
        Err(_) => side_by_side(&focus),
    };
    let outcome = terminals.presentation(workspace.clone(), layout, held);
    let saved = match &mut store {
        Ok(store) => remember(store, workspace, &outcome.layout),
        Err(why) => Err(why.clone()),
    };
    if let Err(why) = saved {
        problems.push(format!("layout not remembered: {why}"));
    }
    SessionOutcome {
        end: outcome.end,
        shown: Some(choice),
        undelivered: outcome.undelivered,
    }
}
