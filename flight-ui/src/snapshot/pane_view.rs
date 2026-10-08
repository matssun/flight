// SPDX-License-Identifier: MIT

use flight_classify::AgentKind;
use flight_state::{AgentState, PaneRef};

/// One agent pane as the UI sees it: identity plus the resolved state and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneView {
    pub pane_ref: PaneRef,
    pub session: String,
    pub window: String,
    pub agent: AgentKind,
    pub state: AgentState,
    /// Short provenance, e.g. `permit.yn` or `finished while away`.
    pub why: String,
    pub title: String,
    /// The pane's process id as last observed. A switch request carries it so the node can
    /// refuse if the pane was replaced since (0: the node predates the field, or not known).
    pub pid: u32,
}
