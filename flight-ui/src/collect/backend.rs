// SPDX-License-Identifier: MIT

use crate::snapshot::{PanePreview, PaneView, UiSnapshot};
use flight_state::PaneRef;

/// Where the dashboard's data comes from. The UI neither knows nor cares whether it is
/// reading tmux directly (the local/SSH [`crate::Collector`]) or an orchestrator that may be
/// on this machine, on the LAN, or hosted: it only sees snapshots, previews and a switch.
pub trait Backend: Send {
    /// The current state of everything watched.
    fn snapshot(&mut self, now: u64) -> UiSnapshot;
    /// The recent screen of one pane.
    fn preview(&mut self, pane: &PaneRef) -> PanePreview;
    /// Take the user to a pane. Backends that cannot say why not.
    fn switch_to(&mut self, pane: &PaneView) -> Result<(), String>;
}
