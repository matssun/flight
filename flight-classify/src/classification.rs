// SPDX-License-Identifier: MIT

use crate::RuleId;
use flight_state::AgentState;

/// A classified state and why: the rule that fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub state: AgentState,
    pub rule_id: RuleId,
}
