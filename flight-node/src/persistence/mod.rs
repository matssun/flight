// SPDX-License-Identifier: MIT

//! The node's side of durable workspaces (ADR-008): the saved file it owns, and the adapters
//! that let `flight-workspaces` observe and start things through this node's tmux servers.

mod node_backend;
mod workspace_persistence;

pub(crate) use node_backend::NodeBackend;
pub use workspace_persistence::WorkspacePersistence;
