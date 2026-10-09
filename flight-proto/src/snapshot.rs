// SPDX-License-Identifier: MIT

use crate::{Incarnation, PaneState, Reject, SavedWorkspace, ServerStatus, Validate};

/// The node's complete externally visible state. Always sufficient on its own: a delta is
/// never required for correctness.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Snapshot {
    #[prost(bytes = "vec", tag = "1")]
    pub incarnation: Vec<u8>,
    #[prost(message, repeated, tag = "2")]
    pub panes: Vec<PaneState>,
    #[prost(message, repeated, tag = "3")]
    pub servers: Vec<ServerStatus>,
    /// Empty from a node that predates saved workspaces (`saved_workspaces_v1`).
    #[prost(message, repeated, tag = "4")]
    pub saved: Vec<SavedWorkspace>,
}

impl Snapshot {
    pub fn incarnation(&self) -> Result<Incarnation, Reject> {
        Incarnation::decode(&self.incarnation)
    }
}

impl Validate for Snapshot {
    fn validate(&self) -> Result<(), Reject> {
        self.incarnation()?;
        self.panes.iter().try_for_each(Validate::validate)?;
        self.servers.iter().try_for_each(Validate::validate)?;
        crate::saved_workspace::validate_list(&self.saved)
    }
}
