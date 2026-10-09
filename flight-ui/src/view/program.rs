// SPDX-License-Identifier: MIT

/// What a new session runs. A closed choice, never typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Claude,
    /// Claude started so that it does not ask before acting. Always spelled out in the form.
    ClaudeSkipPermissions,
    Shell,
}

impl Program {
    pub const ALL: [Program; 3] = [Self::Claude, Self::ClaudeSkipPermissions, Self::Shell];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::ClaudeSkipPermissions => "Claude, no permission prompts",
            Self::Shell => "Shell",
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Claude => Self::ClaudeSkipPermissions,
            Self::ClaudeSkipPermissions => Self::Shell,
            Self::Shell => Self::Claude,
        }
    }

    pub(super) fn prev(self) -> Self {
        match self {
            Self::Claude => Self::Shell,
            Self::ClaudeSkipPermissions => Self::Claude,
            Self::Shell => Self::ClaudeSkipPermissions,
        }
    }
}
