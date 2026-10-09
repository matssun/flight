// SPDX-License-Identifier: MIT

/// What the user did in a saved-workspace prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedPromptInput {
    /// A character typed into the directory field.
    Char(char),
    Backspace,
    /// Tab or an arrow: the other button.
    Next,
    /// Enter: press the focused button.
    Enter,
    /// `y`: confirm.
    Yes,
    /// Esc or `n`: do not.
    Cancel,
}

/// An operation on the selected saved workspace that first asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedOp {
    ChangeRoot,
    Remove,
    AcceptRoot,
    Trust,
}
