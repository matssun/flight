// SPDX-License-Identifier: MIT

/// Agents Flight has a built-in detection manifest for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Claude,
    Codex,
    OpenCode,
    Pi,
    /// Any other agent (aider, gemini, ...). Has no detection manifest: its screen and
    /// title never classify, matching Fleet's empty manifest for unknown agents.
    Other,
}
