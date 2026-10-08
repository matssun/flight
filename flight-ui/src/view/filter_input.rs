// SPDX-License-Identifier: MIT

/// What the user did while typing a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterInput {
    Char(char),
    Backspace,
    /// Stop typing and keep the filter.
    Accept,
    /// Drop the filter.
    Clear,
}
