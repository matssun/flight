// SPDX-License-Identifier: MIT

/// What the user did in the new-session form, independent of which key it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormInput {
    Char(char),
    Backspace,
    Next,
    Prev,
    Left,
    Right,
    /// Activate the focused item: press a button, or move on from a field.
    Enter,
    Cancel,
}
