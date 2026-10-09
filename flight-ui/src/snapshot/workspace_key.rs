// SPDX-License-Identifier: MIT

use flight_state::{HostId, WorkspaceId};

/// A workspace as the dashboard points at it: its id on its host. This, not a row number or a
/// pane, is what a selection or a pending open refers to, so neither follows a surface that
/// the backend replaced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceKey {
    pub host: HostId,
    pub workspace: WorkspaceId,
}
