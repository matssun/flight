// SPDX-License-Identifier: MIT

use crate::{Reject, Validate};

/// Everything a node sends on its one long-lived stream.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct NodeFrame {
    #[prost(oneof = "node_body::Body", tags = "1, 2, 3, 4, 5")]
    pub body: Option<node_body::Body>,
}

pub mod node_body {
    use crate::{Delta, Heartbeat, NodeHello, Response, Snapshot};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    #[allow(clippy::large_enum_variant)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Hello(NodeHello),
        #[prost(message, tag = "2")]
        Heartbeat(Heartbeat),
        #[prost(message, tag = "3")]
        Snapshot(Snapshot),
        #[prost(message, tag = "4")]
        Delta(Delta),
        #[prost(message, tag = "5")]
        Response(Response),
    }
}

impl Validate for NodeFrame {
    fn validate(&self) -> Result<(), Reject> {
        use node_body::Body::*;
        match self
            .body
            .as_ref()
            .ok_or(Reject::Missing("node_frame.body"))?
        {
            Hello(m) => m.validate(),
            Heartbeat(m) => m.validate(),
            Snapshot(m) => m.validate(),
            Delta(m) => m.validate(),
            Response(m) => m.validate(),
        }
    }
}
