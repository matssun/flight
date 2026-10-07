// SPDX-License-Identifier: MIT

/// Why [`NodeLink::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEnd {
    /// Stopped on request.
    Stopped,
    /// This process could not route to the orchestrator for the whole configured window
    /// (immediate "no route"/"network unreachable" errors only: an orchestrator that is down,
    /// slow or refusing never counts). On macOS a long-running process has been seen to stay
    /// in this state after a network interface bounce while a fresh process connects at once;
    /// the caller should exit so that a supervisor restarts it.
    ProcessNetworkUnhealthy,
}
