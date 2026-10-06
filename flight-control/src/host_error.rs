// SPDX-License-Identifier: MIT

use flight_state::{HostId, ServerId};
use std::fmt;

/// Why an operation on a host failed. Typed so a UI can tell a disconnected machine from a
/// machine whose Flight tmux server is simply not running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The registry has no such host.
    UnknownHost(HostId),
    /// The host is registered but has no such tmux server.
    UnknownServer(HostId, ServerId),
    /// The machine could not be reached (DNS, refused, timed out, ssh not runnable).
    HostUnreachable { detail: String },
    /// The machine answered but refused us (key rejected, host key mismatch).
    AuthenticationFailed { detail: String },
    /// tmux is not installed (or not on the PATH) on the host.
    TmuxUnavailable,
    /// tmux is installed but no server is running on the configured endpoint.
    TmuxServerUnavailable,
    /// tmux ran and failed for another reason (e.g. no such pane).
    RemoteCommandFailed { code: Option<i32>, stderr: String },
    /// Invalid configuration (e.g. a malformed SSH alias or socket name).
    InvalidConfig(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHost(h) => write!(f, "unknown host {h}"),
            Self::UnknownServer(h, s) => write!(f, "host {h} has no tmux server {s}"),
            Self::HostUnreachable { detail } => write!(f, "host unreachable: {detail}"),
            Self::AuthenticationFailed { detail } => write!(f, "authentication failed: {detail}"),
            Self::TmuxUnavailable => f.write_str("tmux is not installed on the host"),
            Self::TmuxServerUnavailable => f.write_str("no tmux server on the configured endpoint"),
            Self::RemoteCommandFailed { code, stderr } => {
                write!(f, "tmux failed ({code:?}): {stderr}")
            }
            Self::InvalidConfig(m) => write!(f, "invalid configuration: {m}"),
        }
    }
}

impl std::error::Error for HostError {}
