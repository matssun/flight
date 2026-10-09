// SPDX-License-Identifier: MIT

use crate::snapshot::SurfaceKind;

/// Which surface of a workspace the user is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceChoice {
    Agent,
    Shell,
}

impl SurfaceChoice {
    pub fn is(self, kind: SurfaceKind) -> bool {
        match self {
            Self::Agent => kind.is_agent(),
            Self::Shell => kind == SurfaceKind::Shell,
        }
    }

    /// The name the user knows it by.
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
        }
    }
}
