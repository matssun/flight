// SPDX-License-Identifier: MIT

use super::FormInput;

/// What the user asked for, independent of which key was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    /// Jump to the selected pane.
    Switch,
    ToggleFocus,
    Refresh,
    Quit,
    /// Open the new-session form.
    NewSession,
    /// Something done in the new-session form while it is open.
    Form(FormInput),
}
