// SPDX-License-Identifier: MIT

use flight_proto::ExitReasonCode;
use std::fmt;

/// How a terminal session ended, in words for the person who was using it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEnd {
    /// The user used the local escape.
    UserLeft,
    /// The node reported the end.
    Exited { reason: ExitReasonCode, status: i32 },
    /// The user asked to see the workspace's surfaces side by side.
    Presenting,
    /// The connection failed or broke without a report.
    Lost(String),
}

impl fmt::Display for TerminalEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserLeft => f.write_str("left the terminal"),
            Self::Exited { reason, status } => match reason {
                ExitReasonCode::ClientExited if *status == 0 => {
                    f.write_str("the remote tmux client ended (detached or the session closed)")
                }
                ExitReasonCode::ClientExited => {
                    write!(f, "the remote tmux client ended with status {status}")
                }
                ExitReasonCode::StartFailed => {
                    f.write_str("the pane changed before the terminal started; refresh")
                }
                ExitReasonCode::ClosedByUi => {
                    f.write_str("the terminal was closed (the same pane was opened again)")
                }
                ExitReasonCode::NodeLost => f.write_str("the node's connection was lost"),
                ExitReasonCode::Stalled => {
                    f.write_str("the terminal was too slow to follow and was closed")
                }
                ExitReasonCode::Revoked => f.write_str("this identity is no longer authorized"),
                ExitReasonCode::Shutdown => f.write_str("the orchestrator is shutting down"),
                ExitReasonCode::LeaseExpired => {
                    f.write_str("the terminal's lease lapsed (this UI stopped renewing it)")
                }
                ExitReasonCode::Unspecified => f.write_str("the terminal ended"),
            },
            Self::Presenting => f.write_str("showing the surfaces side by side"),
            Self::Lost(why) => write!(f, "the terminal connection was lost: {why}"),
        }
    }
}
