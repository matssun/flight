// SPDX-License-Identifier: MIT

//! gRPC (tonic) over mutual TLS 1.3 for Flight. This crate only carries frames: all state
//! lives in `flight-node`'s `NodeSession` and `flight-orchestrator`'s `OrchestratorCore`.

mod admin;
mod connector;
mod enroll_client;
mod error;
mod incoming;
mod join;
mod node_link;
mod observe;
mod orchestrator_server;
pub mod outbox;
mod paths;
mod peer;
mod service;
mod shared;
mod ui_client;

pub use error::TransportError;

/// Frames queued per peer before replication is resynchronized instead of buffered.
pub const OUTBOX_CAPACITY: usize = shared::OUTBOX_CAPACITY;
pub use peer::PeerIdentity;

pub use admin::{admin_request, serve_admin};
pub use enroll_client::enroll;
pub use join::{config_path, identity_dir, join, probe, Joined};
pub use node_link::{LinkEnd, LinkLog, NodeLink, NodeLinkConfig};
pub use observe::run_observer;
pub use orchestrator_server::{serve, ServerConfig, ServerControl, ServerHandle};
pub use ui_client::UiClient;
