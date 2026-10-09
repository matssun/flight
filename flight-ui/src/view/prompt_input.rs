// SPDX-License-Identifier: MIT

/// What the user did in the companion-shell prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptInput {
    /// Tab or an arrow: the other button.
    Next,
    /// Enter: press the focused button.
    Enter,
    /// `y`: create.
    Yes,
    /// Esc or `n`: do not create.
    Cancel,
}
