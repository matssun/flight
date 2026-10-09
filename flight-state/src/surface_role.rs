// SPDX-License-Identifier: MIT

/// What a surface is for, as the backend records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    Agent,
    Shell,
}

impl SurfaceRole {
    /// The role a marker names, or `None` for an absent or unknown value (a window that was
    /// never marked, or marked by a newer Flight).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}
