// SPDX-License-Identifier: MIT

/// What a pane/agent is doing. A closed set.
///
/// Deliberately has no `Ord`: sort order and attention are policy (see [`crate::policy`]),
/// not a property of declaration order. Says nothing about how the state was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentState {
    /// Blocked on a tool/permission approval.
    Permit,
    /// Asked the user a question.
    Question,
    /// Turn ended; waiting on the user's next move.
    Done,
    /// Thinking or running tools.
    Busy,
    /// Up, no recent activity.
    Idle,
    /// A plain shell, no agent.
    Shell,
    /// No live process.
    Down,
}

impl AgentState {
    pub const ALL: [AgentState; 7] = [
        Self::Permit,
        Self::Question,
        Self::Done,
        Self::Busy,
        Self::Idle,
        Self::Shell,
        Self::Down,
    ];
}
