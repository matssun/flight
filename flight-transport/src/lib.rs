// SPDX-License-Identifier: MIT

//! gRPC (tonic) over mutual TLS 1.3 for Flight. This crate only carries frames: all state
//! lives in `flight-node`'s `NodeSession` and `flight-orchestrator`'s `OrchestratorCore`.

mod connector;
mod enroll_client;
mod error;
mod incoming;
mod node_link;
mod orchestrator_server;
mod paths;
mod peer;
mod service;
mod shared;
mod ui_client;

pub use error::TransportError;
pub use peer::PeerIdentity;

pub use enroll_client::enroll;
pub use node_link::{NodeLink, NodeLinkConfig};
pub use orchestrator_server::{serve, ServerConfig, ServerHandle};
pub use ui_client::UiClient;
