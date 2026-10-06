// SPDX-License-Identifier: MIT

/// Agents Flight has a built-in detection manifest for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Pi,
}
