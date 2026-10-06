// SPDX-License-Identifier: MIT

//! The hook-less DONE state machine, ported from `resolveDiscoveredStatus` (src/agents/
//! discovery.ts, MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md): a pane that goes working to
//! idle while the user is elsewhere reads Done until viewed or restarted.

use super::Tracking;
use flight_state::AgentState;

/// Advance the machine for one tick. `base` is the fused state; returns the displayed state
/// and the new tracking.
///
/// - Permit/Question hold `was_busy` open (the turn is still in flight), so answering a
///   prompt that ends the turn still lands on Done.
/// - Busy arms the working-to-idle transition.
/// - An idle base consumes the transition: Done if it happened unfocused; cleared by
///   viewing the pane or by work resuming.
pub(super) fn advance(base: AgentState, focused: bool, t: Tracking) -> (AgentState, Tracking) {
    if matches!(
        base,
        AgentState::Permit | AgentState::Question | AgentState::Busy
    ) {
        return (
            base,
            Tracking {
                was_busy: true,
                done: false,
                ..t
            },
        );
    }
    let mut next = t;
    if next.was_busy {
        next.was_busy = false;
        if !focused {
            next.done = true;
        }
    }
    if focused {
        next.done = false; // viewing the pane acknowledges it
    }
    let shown = if next.done {
        AgentState::Done
    } else {
        AgentState::Idle
    };
    (shown, next)
}
