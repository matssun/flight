// SPDX-License-Identifier: MIT

use flight_classify::{detect_agent, AgentKind};
use flight_tmux::PaneInfo;

/// The agent a pane is published as, or `None` when the node does not publish it.
///
/// A pane running a known agent is published as that agent. A pane of a session Flight created
/// is published even when it runs no agent (a plain shell), as `Other`, so a session made from
/// Flight is visible in Flight. Any other pane stays unpublished.
pub(crate) fn pane_agent(info: &PaneInfo) -> Option<AgentKind> {
    detect_agent(&info.current_command).or(info.flight_session.then_some(AgentKind::Other))
}
