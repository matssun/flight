// SPDX-License-Identifier: MIT

use crate::Unavailable;
use flight_classify::AgentKind;
use flight_state::{PaneId, ServerId};

/// One agent pane as an observer saw it, with the captured screen the classifier needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneObservation {
    pub pane: PaneId,
    /// The pane's process id. A different pid under the same pane id is a different pane
    /// (tmux restarted and reused the id), so its Tracking starts fresh.
    pub pid: u32,
    pub agent: AgentKind,
    pub session: String,
    pub window: String,
    pub path: String,
    pub command: String,
    pub title: String,
    pub focused: bool,
    pub screen_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerOutcome {
    /// The complete set of agent panes on the server right now.
    Observed(Vec<PaneObservation>),
    Unavailable(Unavailable),
}

/// The result of observing one tmux server at `now` (seconds since the epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    pub server: ServerId,
    pub now: u64,
    pub outcome: ServerOutcome,
}
