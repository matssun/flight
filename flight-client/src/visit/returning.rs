// SPDX-License-Identifier: MIT

use flight_ui::WorkspaceKey;

/// What the dashboard needs to know when the user comes back from a workspace's terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Returning {
    /// One line for the dashboard's status line: how it ended, what was not delivered, anything
    /// that could not be remembered.
    pub notice: String,
    /// The workspace to be on when the dashboard is back.
    pub select: WorkspaceKey,
}
