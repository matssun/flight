// SPDX-License-Identifier: MIT

use crate::{Incarnation, PaneRefMsg, PaneState, Reject, ServerStatus, Validate};

/// One ordered change to the state a [`crate::Snapshot`] established. Applies only to the
/// snapshot incarnation it names, with `sequence` starting at 1 and increasing by exactly 1.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Delta {
    #[prost(bytes = "vec", tag = "1")]
    pub incarnation: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub sequence: u64,
    #[prost(oneof = "delta_change::Change", tags = "3, 4, 5")]
    pub change: Option<delta_change::Change>,
}

pub mod delta_change {
    use super::{PaneRefMsg, PaneState, ServerStatus};

    /// `PaneState` is the wire message and is held by value everywhere; boxing it here would
    /// change every pattern that matches a delta for no gain at this size.
    #[allow(clippy::large_enum_variant)]
    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Change {
        #[prost(message, tag = "3")]
        PaneUpsert(PaneState),
        #[prost(message, tag = "4")]
        PaneRemoved(PaneRefMsg),
        #[prost(message, tag = "5")]
        ServerStatus(ServerStatus),
    }
}

impl Delta {
    pub fn incarnation(&self) -> Result<Incarnation, Reject> {
        Incarnation::decode(&self.incarnation)
    }
}

impl Validate for Delta {
    fn validate(&self) -> Result<(), Reject> {
        self.incarnation()?;
        if self.sequence == 0 {
            return Err(Reject::OutOfRange("delta.sequence"));
        }
        match self
            .change
            .as_ref()
            .ok_or(Reject::Missing("delta.change"))?
        {
            delta_change::Change::PaneUpsert(p) => p.validate(),
            delta_change::Change::PaneRemoved(r) => r.validate(),
            delta_change::Change::ServerStatus(s) => s.validate(),
        }
    }
}
