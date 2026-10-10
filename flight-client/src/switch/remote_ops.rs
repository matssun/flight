// SPDX-License-Identifier: MIT

use super::RemoteError;
use flight_state::PaneRef;

/// What a switch needs from the orchestrator for a pane on another machine. Each call is one
/// request over Flight's own connection; nothing here knows where the node is.
pub trait RemoteOps {
    /// Ask the pane's node to select the pane, if it still runs the process the user saw. Only
    /// for a node that cannot show a terminal: a terminal selects the pane in its own view.
    fn reveal(&mut self, pane: &PaneRef, pid: u32) -> Result<(), String>;

    /// Ask for a terminal onto the pane, guarded by the process the user saw; returns the
    /// terminal id both ends attach with.
    fn open_terminal(&mut self, pane: &PaneRef, pid: u32) -> Result<Vec<u8>, RemoteError>;
}
