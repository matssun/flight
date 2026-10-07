// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{AgentKindCode, PaneRefMsg, Reject, SourceCode, StateCode, Validate};
use flight_state::AgentState;

/// One agent pane's resolved, externally visible state. No terminal contents.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct PaneState {
    #[prost(message, optional, tag = "1")]
    pub pane_ref: Option<PaneRefMsg>,
    #[prost(enumeration = "AgentKindCode", tag = "2")]
    pub agent_kind: i32,
    #[prost(enumeration = "StateCode", tag = "3")]
    pub state: i32,
    #[prost(enumeration = "SourceCode", tag = "4")]
    pub source: i32,
    /// The winning rule id, when a rule decided it (provenance summary).
    #[prost(string, tag = "5")]
    pub rule_id: String,
    /// A short human reason, e.g. `finished while away`.
    #[prost(string, tag = "6")]
    pub why: String,
    // Tag 7 (observed_at) and tag 13 (title) are retired: both change on every poll and would
    // turn every refresh into a delta. Liveness comes from heartbeats and snapshots.
    /// Seconds since the epoch at which `state` last changed.
    #[prost(uint64, tag = "8")]
    pub changed_at: u64,
    #[prost(string, tag = "9")]
    pub session: String,
    #[prost(string, tag = "10")]
    pub window: String,
    #[prost(string, tag = "11")]
    pub path: String,
    #[prost(string, tag = "12")]
    pub command: String,
    /// The pane's process id: the pane incarnation a control action must name. Changes only
    /// when the process does, so it adds no per-poll delta. 0 from a node that predates it.
    #[prost(uint32, tag = "14")]
    pub pid: u32,
}

impl PaneState {
    pub fn agent_state(&self) -> Result<AgentState, Reject> {
        Ok(match StateCode::decode(self.state, "pane_state.state")? {
            StateCode::Permit => AgentState::Permit,
            StateCode::Question => AgentState::Question,
            StateCode::Done => AgentState::Done,
            StateCode::Busy => AgentState::Busy,
            StateCode::Idle => AgentState::Idle,
            StateCode::Shell => AgentState::Shell,
            // `decode` already refused Unspecified.
            StateCode::Down | StateCode::Unspecified => AgentState::Down,
        })
    }

    pub fn state_code(state: AgentState) -> StateCode {
        match state {
            AgentState::Permit => StateCode::Permit,
            AgentState::Question => StateCode::Question,
            AgentState::Done => StateCode::Done,
            AgentState::Busy => StateCode::Busy,
            AgentState::Idle => StateCode::Idle,
            AgentState::Shell => StateCode::Shell,
            AgentState::Down => StateCode::Down,
        }
    }
}

impl Validate for PaneState {
    fn validate(&self) -> Result<(), Reject> {
        self.pane_ref
            .as_ref()
            .ok_or(Reject::Missing("pane_state.pane_ref"))?
            .validate()?;
        AgentKindCode::decode(self.agent_kind, "pane_state.agent_kind")?;
        self.agent_state()?;
        SourceCode::decode(self.source, "pane_state.source")?;
        non_empty(&self.session, "pane_state.session")
    }
}
