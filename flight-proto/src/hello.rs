// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{ProtocolVersion, Reject, Validate};

/// First frame a node sends. `node_id` is the key fingerprint; `display_name` is mutable.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct NodeHello {
    #[prost(message, optional, tag = "1")]
    pub version: Option<ProtocolVersion>,
    #[prost(string, tag = "2")]
    pub node_id: String,
    #[prost(string, tag = "3")]
    pub display_name: String,
    #[prost(string, repeated, tag = "4")]
    pub capabilities: Vec<String>,
    /// The tmux servers this node observes (their `ServerId`s).
    #[prost(string, repeated, tag = "5")]
    pub servers: Vec<String>,
}

impl Validate for NodeHello {
    fn validate(&self) -> Result<(), Reject> {
        self.version.ok_or(Reject::Missing("node_hello.version"))?;
        non_empty(&self.node_id, "node_hello.node_id")
    }
}

/// The orchestrator's answer: the agreed version and the capabilities it accepted.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct OrchestratorHello {
    #[prost(message, optional, tag = "1")]
    pub version: Option<ProtocolVersion>,
    #[prost(string, repeated, tag = "2")]
    pub accepted_capabilities: Vec<String>,
    #[prost(uint32, tag = "3")]
    pub heartbeat_interval_secs: u32,
}

impl Validate for OrchestratorHello {
    fn validate(&self) -> Result<(), Reject> {
        self.version
            .ok_or(Reject::Missing("orchestrator_hello.version"))?;
        if self.heartbeat_interval_secs == 0 {
            return Err(Reject::OutOfRange(
                "orchestrator_hello.heartbeat_interval_secs",
            ));
        }
        Ok(())
    }
}
