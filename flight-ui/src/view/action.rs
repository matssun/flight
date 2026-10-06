// SPDX-License-Identifier: MIT

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
}
