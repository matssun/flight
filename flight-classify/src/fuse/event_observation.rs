// SPDX-License-Identifier: MIT

use flight_state::AgentState;

/// State derived from the event log, and when its last event happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventObservation {
    pub state: AgentState,
    /// Seconds since the epoch of the last event.
    pub ts: u64,
}
