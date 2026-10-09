// SPDX-License-Identifier: MIT

/// Session option: the workspace a session realizes. Set on every session Flight creates.
pub const WORKSPACE_OPTION: &str = "@flight_workspace";
/// Window option: what the window's pane is for (`agent` or `shell`).
pub const SURFACE_OPTION: &str = "@flight_surface";
/// Window option: the surface's own id.
pub const SURFACE_ID_OPTION: &str = "@flight_surface_id";

/// What a surface is, as recorded in the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceTag {
    Agent,
    Shell,
}

impl SurfaceTag {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
        }
    }

    /// The tag a window carries, or `None` for an absent or unknown value (a window that was
    /// never marked, or marked by a newer Flight).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

/// The identity Flight records in the backend so a workspace and its surfaces are found again
/// after a restart of the node, the orchestrator or the UI. The ids are Flight's, not tmux's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceMark {
    pub workspace_id: String,
    pub surface_id: String,
    pub kind: SurfaceTag,
}
