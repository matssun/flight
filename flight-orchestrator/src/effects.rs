// SPDX-License-Identifier: MIT

use crate::TerminalId;
use crate::{ConnId, UiId};
use flight_proto::{ExitReasonCode, OrchestratorFrame, UiEvent};

/// What the caller must carry out after an event.
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub to_nodes: Vec<(ConnId, OrchestratorFrame)>,
    pub to_ui: Vec<(UiId, UiEvent)>,
    /// Connections to drop, with the reason (for logs).
    pub close: Vec<(ConnId, String)>,
    /// One-line operator notes about liveness (why and when a node changed state, with ages),
    /// for the log. They never require action.
    pub notes: Vec<String>,
    /// Terminals the core has just ended, for the transport to close their streams.
    pub terminals_ended: Vec<(TerminalId, ExitReasonCode)>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.to_nodes.is_empty()
            && self.to_ui.is_empty()
            && self.close.is_empty()
            && self.terminals_ended.is_empty()
    }
}
