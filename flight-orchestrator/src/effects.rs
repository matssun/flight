// SPDX-License-Identifier: MIT

use crate::{ConnId, UiId};
use flight_proto::{OrchestratorFrame, UiEvent};

/// What the caller must carry out after an event.
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub to_nodes: Vec<(ConnId, OrchestratorFrame)>,
    pub to_ui: Vec<(UiId, UiEvent)>,
    /// Connections to drop, with the reason (for logs).
    pub close: Vec<(ConnId, String)>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.to_nodes.is_empty() && self.to_ui.is_empty() && self.close.is_empty()
    }
}
