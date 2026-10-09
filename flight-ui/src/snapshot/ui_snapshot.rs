// SPDX-License-Identifier: MIT

use super::{HostView, SavedView};

/// Everything the UI needs to draw one frame of data. Built by the collector; the view
/// model and renderer never perform I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiSnapshot {
    pub hosts: Vec<HostView>,
    /// Workspaces the nodes have saved, running or not.
    pub saved: Vec<SavedView>,
    /// Seconds since the epoch.
    pub taken_at: u64,
}
