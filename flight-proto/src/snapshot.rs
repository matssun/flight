// SPDX-License-Identifier: MIT

use crate::{PaneState, Reject, ServerStatus, Validate};

/// The node's complete externally visible state. Always sufficient on its own: a delta is
/// never required for correctness.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Snapshot {
    #[prost(uint64, tag = "1")]
    pub generation: u64,
    #[prost(message, repeated, tag = "2")]
    pub panes: Vec<PaneState>,
    #[prost(message, repeated, tag = "3")]
    pub servers: Vec<ServerStatus>,
}

impl Validate for Snapshot {
    fn validate(&self) -> Result<(), Reject> {
        self.panes.iter().try_for_each(Validate::validate)?;
        self.servers.iter().try_for_each(Validate::validate)
    }
}
