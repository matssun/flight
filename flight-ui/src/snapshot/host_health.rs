// SPDX-License-Identifier: MIT

use flight_control::HostError;

/// What the UI shows for a host, derived from whether its pane listing succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostHealth {
    Online,
    /// Connected, but the orchestrator has not heard from the node lately: panes are last-known.
    Stale,
    /// The orchestrator has lost the node: panes are last-known.
    Disconnected,
    /// tmux is there but no Flight server is running on the endpoint.
    NoServer,
    Unreachable(String),
    AuthFailed(String),
    NoTmux,
    Failed(String),
}

impl HostHealth {
    pub fn from_error(e: &HostError) -> Self {
        match e {
            HostError::TmuxServerUnavailable => Self::NoServer,
            HostError::HostUnreachable { detail } => Self::Unreachable(detail.clone()),
            HostError::AuthenticationFailed { detail } => Self::AuthFailed(detail.clone()),
            HostError::TmuxUnavailable => Self::NoTmux,
            other => Self::Failed(other.to_string()),
        }
    }
}
