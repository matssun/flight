// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{Incarnation, NodeStatusCode, PaneRefMsg, PaneState, Reject, ServerStatus, Validate};

/// One node as the orchestrator presents it to a UI.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct NodeView {
    #[prost(string, tag = "1")]
    pub node_id: String,
    #[prost(string, tag = "2")]
    pub display_name: String,
    #[prost(enumeration = "NodeStatusCode", tag = "3")]
    pub status: i32,
    #[prost(message, repeated, tag = "4")]
    pub servers: Vec<ServerStatus>,
    #[prost(message, repeated, tag = "5")]
    pub panes: Vec<PaneState>,
}

impl Validate for NodeView {
    fn validate(&self) -> Result<(), Reject> {
        non_empty(&self.node_id, "node_view.node_id")?;
        NodeStatusCode::decode(self.status, "node_view.status")?;
        self.servers.iter().try_for_each(Validate::validate)?;
        self.panes.iter().try_for_each(Validate::validate)
    }
}

/// The whole fleet as the orchestrator currently knows it. Authoritative on its own.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct FleetSnapshot {
    #[prost(bytes = "vec", tag = "1")]
    pub incarnation: Vec<u8>,
    #[prost(message, repeated, tag = "2")]
    pub nodes: Vec<NodeView>,
}

impl FleetSnapshot {
    pub fn incarnation(&self) -> Result<Incarnation, Reject> {
        Incarnation::decode(&self.incarnation)
    }
}

impl Validate for FleetSnapshot {
    fn validate(&self) -> Result<(), Reject> {
        self.incarnation()?;
        self.nodes.iter().try_for_each(Validate::validate)
    }
}

#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct NodeStatusChanged {
    #[prost(string, tag = "1")]
    pub node_id: String,
    #[prost(enumeration = "NodeStatusCode", tag = "2")]
    pub status: i32,
}

/// An ordered change to a [`FleetSnapshot`]; same incarnation/sequence rules as a node delta.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct FleetDelta {
    #[prost(bytes = "vec", tag = "1")]
    pub incarnation: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub sequence: u64,
    #[prost(oneof = "fleet_change::Change", tags = "3, 4, 5, 6")]
    pub change: Option<fleet_change::Change>,
}

pub mod fleet_change {
    use super::{NodeStatusChanged, NodeView, PaneRefMsg, PaneState};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    #[allow(clippy::large_enum_variant)]
    pub enum Change {
        /// Replaces the whole node entry, including its panes.
        #[prost(message, tag = "3")]
        NodeUpsert(NodeView),
        #[prost(message, tag = "4")]
        NodeStatus(NodeStatusChanged),
        #[prost(message, tag = "5")]
        PaneUpsert(PaneState),
        #[prost(message, tag = "6")]
        PaneRemoved(PaneRefMsg),
    }
}

impl FleetDelta {
    pub fn incarnation(&self) -> Result<Incarnation, Reject> {
        Incarnation::decode(&self.incarnation)
    }
}

impl Validate for FleetDelta {
    fn validate(&self) -> Result<(), Reject> {
        self.incarnation()?;
        if self.sequence == 0 {
            return Err(Reject::OutOfRange("fleet_delta.sequence"));
        }
        match self
            .change
            .as_ref()
            .ok_or(Reject::Missing("fleet_delta.change"))?
        {
            fleet_change::Change::NodeUpsert(n) => n.validate(),
            fleet_change::Change::NodeStatus(s) => {
                non_empty(&s.node_id, "node_status.node_id")?;
                NodeStatusCode::decode(s.status, "node_status.status").map(|_| ())
            }
            fleet_change::Change::PaneUpsert(p) => p.validate(),
            fleet_change::Change::PaneRemoved(r) => r.validate(),
        }
    }
}
