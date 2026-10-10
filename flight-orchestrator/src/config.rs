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
    pub terminal_limits: TerminalLimits,
    /// After a node answered an open, how long both ends have to attach their streams.
    pub terminal_attach_window_secs: u64,
    /// How long an attached terminal lives after its UI's last lease (ADR-004).
    pub terminal_lease_ttl_secs: u64,
}

/// How many terminals may be open at once. Beyond a limit an open fails at once with `Busy`
/// and nothing is created. A presentation holds one terminal per tile on screen, and a terminal
/// that is closing still counts toward the node's and the total until it is gone; the defaults
/// leave room for eight tiles on each of two interfaces at one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalLimits {
    pub per_node: usize,
    pub per_ui: usize,
    pub total: usize,
}

impl Default for TerminalLimits {
    fn default() -> Self {
        Self {
            per_node: 16,
            per_ui: 8,
            total: 64,
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_secs: 5,
            stale_after_intervals: 3,
            disconnect_after_intervals: 6,
            request_timeout_secs: 10,
            terminal_limits: TerminalLimits::default(),
            terminal_attach_window_secs: 10,
            terminal_lease_ttl_secs: 15,
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
