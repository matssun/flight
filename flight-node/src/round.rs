// SPDX-License-Identifier: MIT

use crate::Unavailable;
use flight_classify::AgentKind;
use flight_state::{PaneId, RawPlacement, ServerId};
use flight_tmux::PaneInfo;

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
    /// Where the backend says the pane belongs (workspace, surface, root).
    pub placement: RawPlacement,
}

impl PaneObservation {
    pub(crate) fn from_info(
        info: PaneInfo,
        agent: AgentKind,
        screen_lines: Vec<String>,
        focused: bool,
    ) -> Self {
        let placement = raw_placement(&info);
        Self {
            pane: PaneId::new(info.pane_id),
            pid: info.pane_pid,
            agent,
            session: info.session_name,
            window: info.window_name,
            path: info.current_path,
            command: info.current_command,
            title: info.pane_title,
            focused,
            screen_lines,
            placement,
        }
    }
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

/// What the backend recorded about the pane's place, as read from the listing.
pub(crate) fn raw_placement(info: &PaneInfo) -> RawPlacement {
    RawPlacement {
        workspace_id: info.workspace_id.clone(),
        surface_id: info.surface_id.clone(),
        surface_kind: info.surface_kind.clone(),
        window_id: info.window_id.clone(),
        session_id: info.session_id.clone(),
        session_path: info.session_path.clone(),
    }
}
