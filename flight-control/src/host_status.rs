// SPDX-License-Identifier: MIT

use crate::HostError;

/// What a UI needs to render a host row before the user presses a key.
///
/// `reachable` means commands can be executed on the host: it is false for an unreachable
/// machine and for one that refused authentication (see `problem` to tell them apart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStatus {
    pub reachable: bool,
    pub tmux_available: bool,
    /// A tmux server is running on the configured endpoint.
    pub endpoint_available: bool,
    pub tmux_version: Option<String>,
    /// Why the first failed check failed, if any. `None` for a fully online host.
    pub problem: Option<HostError>,
}

impl HostStatus {
    pub fn is_online(&self) -> bool {
        self.reachable && self.tmux_available && self.endpoint_available
    }
}
