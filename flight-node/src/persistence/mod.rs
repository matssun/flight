// SPDX-License-Identifier: MIT

//! The node's side of durable workspaces (ADR-008): the saved file it owns, and the adapters
//! that let `flight-workspaces` observe and start things through this node's tmux servers.

mod node_backend;
mod saved_action_request;
mod saved_report;
mod workspace_persistence;

pub(crate) use node_backend::NodeBackend;
pub use saved_action_request::{SavedAction, SavedActionRequest};
pub use workspace_persistence::WorkspacePersistence;
