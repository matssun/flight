// SPDX-License-Identifier: MIT

use flight_state::{needs_attention, AgentState};

/// The group a session is listed under: how much it wants the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Waiting for approval, asking a question, or finished and waiting for the next move.
    NeedsYou,
    /// Working on its own.
    Working,
    /// Idle, a plain shell, or down.
    Quiet,
}

impl Tier {
    pub fn of(state: AgentState) -> Self {
        if needs_attention(state) {
            Self::NeedsYou
        } else if state == AgentState::Busy {
            Self::Working
        } else {
            Self::Quiet
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsYou => "Needs you",
            Self::Working => "Working",
            Self::Quiet => "Quiet",
        }
    }
}
