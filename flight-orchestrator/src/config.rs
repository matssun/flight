// SPDX-License-Identifier: MIT

/// Timing policy. Defaults suit a LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestratorConfig {
    pub heartbeat_interval_secs: u32,
    /// A node silent for this many heartbeat intervals is `Stale`.
    pub stale_after_intervals: u32,
    /// A node silent for this many intervals is dropped and `Disconnected`.
    pub disconnect_after_intervals: u32,
    /// A routed request with no response by then fails with `NodeUnreachable`.
    pub request_timeout_secs: u64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_secs: 5,
            stale_after_intervals: 3,
            disconnect_after_intervals: 6,
            request_timeout_secs: 10,
        }
    }
}

impl OrchestratorConfig {
    pub(crate) fn stale_after(&self) -> u64 {
        u64::from(self.heartbeat_interval_secs) * u64::from(self.stale_after_intervals)
    }

    pub(crate) fn disconnect_after(&self) -> u64 {
        u64::from(self.heartbeat_interval_secs) * u64::from(self.disconnect_after_intervals)
    }
}
