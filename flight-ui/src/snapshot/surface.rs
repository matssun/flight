// SPDX-License-Identifier: MIT

use super::{PaneView, SurfaceKind};
use flight_state::{SurfaceId, WorkspaceId};

/// One resource attached to a workspace: its agent or its shell. It belongs to exactly one
/// workspace, and its identity is its own id, not the pane the backend currently uses for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub id: SurfaceId,
    pub workspace: WorkspaceId,
    pub kind: SurfaceKind,
    pub pane: PaneView,
}

impl From<&PaneView> for Surface {
    fn from(pane: &PaneView) -> Self {
        Self {
            id: pane.surface.clone(),
            workspace: pane.workspace.clone(),
            kind: pane.kind,
            pane: pane.clone(),
        }
    }
}
