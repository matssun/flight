// SPDX-License-Identifier: MIT

//! Presentation-independent policy over [`AgentState`]: who needs the human, and in what
//! order states are listed. Kept apart from the enum so either can change without touching
//! the state representation.

use crate::AgentState;

/// Does the human need to act on an agent in this state?
pub fn needs_attention(state: AgentState) -> bool {
    match state {
        AgentState::Permit | AgentState::Question | AgentState::Done => true,
        AgentState::Busy | AgentState::Idle | AgentState::Shell | AgentState::Down => false,
    }
}

/// Listing order, most urgent first (lower sorts earlier). Every attention state ranks
/// above every non-attention state.
pub fn sort_rank(state: AgentState) -> u8 {
    match state {
        AgentState::Permit => 0,
        AgentState::Question => 1,
        AgentState::Done => 2,
        AgentState::Busy => 3,
        AgentState::Idle => 4,
        AgentState::Shell => 5,
        AgentState::Down => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_are_unique() {
        let mut ranks: Vec<u8> = AgentState::ALL.iter().map(|s| sort_rank(*s)).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), AgentState::ALL.len());
    }

    #[test]
    fn attention_states_rank_before_all_others() {
        let worst_attention = AgentState::ALL
            .iter()
            .filter(|s| needs_attention(**s))
            .map(|s| sort_rank(*s))
            .max();
        let best_other = AgentState::ALL
            .iter()
            .filter(|s| !needs_attention(**s))
            .map(|s| sort_rank(*s))
            .min();
        assert!(worst_attention < best_other);
    }

    #[test]
    fn exactly_the_user_turn_states_need_attention() {
        let set: Vec<AgentState> = AgentState::ALL
            .into_iter()
            .filter(|s| needs_attention(*s))
            .collect();
        assert_eq!(
            set,
            [AgentState::Permit, AgentState::Question, AgentState::Done]
        );
    }

    #[test]
    fn sorting_by_rank_orders_most_urgent_first() {
        let mut v = vec![
            AgentState::Down,
            AgentState::Busy,
            AgentState::Permit,
            AgentState::Done,
        ];
        v.sort_by_key(|s| sort_rank(*s));
        assert_eq!(
            v,
            [
                AgentState::Permit,
                AgentState::Done,
                AgentState::Busy,
                AgentState::Down
            ]
        );
    }
}
