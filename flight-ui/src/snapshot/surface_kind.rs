// SPDX-License-Identifier: MIT

use flight_classify::AgentKind;

/// What a surface of a workspace is. Which agent (Claude, Codex, ...) is an implementation of
/// the Agent surface; it does not define the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Agent(AgentKind),
    Shell,
}

impl SurfaceKind {
    pub fn is_agent(self) -> bool {
        matches!(self, Self::Agent(_))
    }
}
