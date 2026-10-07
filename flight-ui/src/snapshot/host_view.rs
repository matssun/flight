// SPDX-License-Identifier: MIT

use super::{HostHealth, PaneView};
use flight_state::{HostId, ServerId};

/// One (host, server) endpoint: its health and, when online, its agent panes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostView {
    pub host: HostId,
    /// What to show for the host: its display name. Identity stays `host`.
    pub label: String,
    pub server: ServerId,
    pub health: HostHealth,
    pub panes: Vec<PaneView>,
}
