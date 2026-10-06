// SPDX-License-Identifier: MIT

/// Hook event names in an agent's JSONL event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    PreToolUse,
    Stop,
    SubagentStop,
    Notification,
    /// Written by the dashboard when the user has seen a finished turn.
    Acknowledged,
    /// Any event name Flight does not interpret.
    Other,
}

impl EventKind {
    pub fn from_wire(name: &str) -> Self {
        match name {
            "PreToolUse" => Self::PreToolUse,
            "Stop" => Self::Stop,
            "SubagentStop" => Self::SubagentStop,
            "Notification" => Self::Notification,
            "Acknowledged" => Self::Acknowledged,
            _ => Self::Other,
        }
    }
}
