// SPDX-License-Identifier: MIT

/// What a new workspace starts. A closed choice, never typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Claude,
    /// Claude started so that it does not ask before acting. Always spelled out in the form.
    ClaudeSkipPermissions,
    /// A workspace made of a shell alone. The form does not offer it (a workspace is made for
    /// an agent, and gets its shell afterwards); the wire still carries it for other callers.
    Shell,
}

impl Program {
    /// What the new-workspace form offers: the agents.
    pub const ALL: [Program; 2] = [Self::Claude, Self::ClaudeSkipPermissions];

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
            Self::ClaudeSkipPermissions | Self::Shell => Self::Claude,
        }
    }

    pub(super) fn prev(self) -> Self {
        self.next()
    }
}
