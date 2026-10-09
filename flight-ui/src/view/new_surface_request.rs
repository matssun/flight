// SPDX-License-Identifier: MIT

use super::SurfaceChoice;
use flight_state::{HostId, WorkspaceId};

/// A request to add a surface to a workspace that exists. It names the workspace and the kind
/// and nothing else: the host and the directory are the workspace's own, so there is nothing
/// to re-enter (and nothing a caller could point somewhere else). `host` and `name` are for
/// messages and for finding the workspace again; they are not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSurfaceRequest {
    pub host: HostId,
    pub workspace: WorkspaceId,
    pub name: String,
    pub host_label: String,
    pub kind: SurfaceChoice,
}
