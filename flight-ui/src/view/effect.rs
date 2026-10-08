// SPDX-License-Identifier: MIT

use crate::PaneView;
use flight_state::PaneRef;

/// What the terminal loop must do after the view model handled an action. The view model
/// performs no I/O; it only asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    Refresh,
    /// The selection changed: tell the collector which pane to preview.
    Select(Option<PaneRef>),
    /// Take the user to this pane, exactly as it looked when they asked.
    Switch(PaneView),
}
