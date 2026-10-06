// SPDX-License-Identifier: MIT

use flight_state::AgentState;

/// The state an agent's hook status file last reported, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookObservation {
    pub state: AgentState,
    /// Seconds since the epoch.
    pub ts: u64,
}

impl HookObservation {
    /// Map a hook status-file state word to a state: `permit` and `waiting` are Permit,
    /// `question` Question, `done` and `completed` Done, `working` Busy, anything else Idle.
    pub fn from_wire(state: &str, ts: u64) -> Self {
        let state = match state {
            "permit" | "waiting" => AgentState::Permit,
            "question" => AgentState::Question,
            "done" | "completed" => AgentState::Done,
            "working" => AgentState::Busy,
            _ => AgentState::Idle,
        };
        Self { state, ts }
    }
}
