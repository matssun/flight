// SPDX-License-Identifier: MIT

/// Where keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Dashboard,
    /// Typing a search.
    Search,
    Form,
    /// The companion-shell prompt.
    Prompt,
    Help,
    /// A saved-workspace question with buttons.
    SavedConfirm,
    /// A saved-workspace question with a directory to type.
    SavedInput,
}
