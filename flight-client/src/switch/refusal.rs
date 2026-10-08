// SPDX-License-Identifier: MIT

use std::fmt;

/// Why a switch was not attempted. Nothing has been changed when one of these is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The node did not report the pane's process, so a stale pane cannot be detected.
    UnknownProcess,
    /// The pane or server name is not in the shape tmux hands out.
    BadTarget(&'static str),
    /// This dashboard runs inside a different tmux server than the pane; nesting is the
    /// user's choice, never ours.
    OtherTmuxServer { ui: Option<String>, pane: String },
    /// Inside the pane's tmux server, but no terminal is attached to the dashboard's session.
    NoClient,
    /// Several terminals show the dashboard's session; tmux cannot say which one is this.
    AmbiguousClients(usize),
    /// Remote panes are shown through a terminal session over Flight (ADR-003), which the
    /// dashboard does not have yet.
    RemoteTerminalUnavailable,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProcess => {
                f.write_str("the node does not report this pane's process (older node); upgrade it")
            }
            Self::BadTarget(what) => write!(f, "unusable {what}"),
            Self::OtherTmuxServer { ui, pane } => write!(
                f,
                "this dashboard runs inside tmux server {}, the pane is on server {pane}; \
                 attach manually to nest",
                ui.as_deref().unwrap_or("(unnamed)")
            ),
            Self::NoClient => f.write_str(
                "no tmux client is attached to this dashboard's session; attach manually",
            ),
            Self::AmbiguousClients(n) => write!(
                f,
                "cannot tell which of {n} tmux clients shows this dashboard; attach manually"
            ),
            Self::RemoteTerminalUnavailable => f.write_str(
                "showing a pane of another machine is not available yet in this dashboard",
            ),
        }
    }
}
