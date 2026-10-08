// SPDX-License-Identifier: MIT

/// What a new session runs. A closed choice, never typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Claude,
    Shell,
}

impl Program {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Shell => "Shell",
        }
    }

    pub(super) fn other(self) -> Self {
        match self {
            Self::Claude => Self::Shell,
            Self::Shell => Self::Claude,
        }
    }
}
