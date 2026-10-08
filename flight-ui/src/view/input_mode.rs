// SPDX-License-Identifier: MIT

/// Where keys go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Dashboard,
    /// Typing a search.
    Search,
    Form,
    Help,
}
