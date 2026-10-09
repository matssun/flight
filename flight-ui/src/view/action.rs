// SPDX-License-Identifier: MIT

use super::{FilterInput, FormInput, PromptInput, SurfaceChoice};
use flight_state::PaneRef;

/// What the user asked for, independent of which key was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    /// Open the selected workspace: its agent, or its shell if it has no agent (Enter).
    Switch,
    /// Open this surface of the selected workspace (`a` agent, `s` shell).
    Open(SurfaceChoice),
    /// Point at this session (a click).
    Select(PaneRef),
    Refresh,
    Quit,
    /// Esc: drop the search if there is one, otherwise quit.
    Back,
    /// Open the new-session form.
    NewSession,
    /// Something done in the new-session form while it is open.
    Form(FormInput),
    /// Something done in the companion-shell prompt while it is open.
    Prompt(PromptInput),
    /// Start typing a search.
    Search,
    /// Something done while typing a search, or to drop one.
    Filter(FilterInput),
    Help,
    CloseHelp,
}
