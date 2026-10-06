// SPDX-License-Identifier: MIT

use super::Tracking;
use crate::FusedClassification;
use flight_state::AgentState;

/// How a resolved state came about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The fusion result for this tick's evidence.
    pub fused: FusedClassification,
    /// The displayed Done was synthesized by the temporal machine (finished while the user
    /// was elsewhere), not read from any evidence.
    pub synthesized_done: bool,
}

/// The outcome of one resolution, and the memory to feed into the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedState {
    pub state: AgentState,
    pub provenance: Provenance,
    pub tracking: Tracking,
    /// Seconds since the epoch.
    pub observed_at: u64,
}
