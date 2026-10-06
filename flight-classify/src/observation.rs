// SPDX-License-Identifier: MIT

/// Raw evidence about one pane, as gathered by someone else. Deliberately minimal: each
/// field added here becomes part of the classifier contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// Captured screen, one entry per line. ANSI escapes are allowed; they are stripped
    /// before matching.
    pub screen_lines: Vec<String>,
    /// The pane title (`#{pane_title}`).
    pub title: String,
}
