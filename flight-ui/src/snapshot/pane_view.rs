// SPDX-License-Identifier: MIT

use super::SurfaceKind;
use flight_classify::AgentKind;
use flight_state::{AgentState, PaneRef, SurfaceId, WorkspaceId};

/// One surface's pane as the UI sees it: identity plus the resolved state and why, and the
/// workspace and surface it belongs to.
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
    /// The workspace this pane is a surface of, and which surface. Identity: the session name
    /// above is only what the workspace is called.
    pub workspace: WorkspaceId,
    pub surface: SurfaceId,
    pub kind: SurfaceKind,
    /// The workspace's root directory on its host.
    pub root: String,
}
