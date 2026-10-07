// SPDX-License-Identifier: MIT

use crate::{Reject, Validate};

/// Start receiving the fleet: the orchestrator answers with a `FleetSnapshot` then deltas.
#[derive(Clone, Copy, PartialEq, Eq, prost::Message)]
pub struct Subscribe {}

/// What a UI sends the orchestrator. A separate interface from the node protocol: a UI
/// consumes state and issues operator commands; it can never publish node state.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct UiRequest {
    #[prost(oneof = "ui_request_body::Body", tags = "1, 2")]
    pub body: Option<ui_request_body::Body>,
}

pub mod ui_request_body {
    use crate::{Request, Subscribe};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Subscribe(Subscribe),
        #[prost(message, tag = "2")]
        Command(Request),
    }
}

impl Validate for UiRequest {
    fn validate(&self) -> Result<(), Reject> {
        match self
            .body
            .as_ref()
            .ok_or(Reject::Missing("ui_request.body"))?
        {
            ui_request_body::Body::Subscribe(_) => Ok(()),
            ui_request_body::Body::Command(r) => r.validate(),
        }
    }
}

/// What the orchestrator sends a UI.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct UiEvent {
    #[prost(oneof = "ui_event_body::Body", tags = "1, 2, 3")]
    pub body: Option<ui_event_body::Body>,
}

pub mod ui_event_body {
    use crate::{FleetDelta, FleetSnapshot, Response};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    #[allow(clippy::large_enum_variant)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Snapshot(FleetSnapshot),
        #[prost(message, tag = "2")]
        Delta(FleetDelta),
        #[prost(message, tag = "3")]
        Response(Response),
    }
}

impl Validate for UiEvent {
    fn validate(&self) -> Result<(), Reject> {
        match self.body.as_ref().ok_or(Reject::Missing("ui_event.body"))? {
            ui_event_body::Body::Snapshot(s) => s.validate(),
            ui_event_body::Body::Delta(d) => d.validate(),
            ui_event_body::Body::Response(r) => r.validate(),
        }
    }
}
