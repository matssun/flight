// SPDX-License-Identifier: MIT

use crate::Round;

/// How a node looks at its tmux servers, one [`Round`] per server per call. Implementations
/// differ in cost, never in what they report: the sequential observer is the reference, and
/// the others must agree with it.
pub trait PaneObserver: Send {
    fn observe(&mut self, now: u64) -> Vec<Round>;

    /// Things the operator should know since the last call (a strategy degraded to its
    /// fallback, or recovered). Reported on transitions, not every round.
    fn take_notes(&mut self) -> Vec<String> {
        Vec::new()
    }
}
