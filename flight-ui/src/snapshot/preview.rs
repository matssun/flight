// SPDX-License-Identifier: MIT

use flight_state::PaneRef;

/// The captured screen of one pane, or why it could not be captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanePreview {
    pub pane: PaneRef,
    pub content: Result<Vec<String>, String>,
}
