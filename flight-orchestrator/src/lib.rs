// SPDX-License-Identifier: MIT

//! flight-orchestrator: the live control plane, as a pure state machine.
//!
//! ```text
//! node frames -> ReplicationCursor per node -> NodeImage -> OrchestratorCore -> FleetSnapshot/FleetDelta
//! ```
//!
//! No sockets, TLS, async or persistence: callers feed it events (a connection, a frame,
//! a disconnect, a tick, a UI request) with `now`, and it returns [`Effects`] to carry out.
//!
//! Invariants:
//! - It holds only live state reconstructible from connected nodes; it may vanish without
//!   affecting any workload.
//! - Every node image visible to a UI is a prefix of an accepted node replication stream: a
//!   delta is validated against incarnation and sequence before it touches the image, and a
//!   gap keeps the last consistent image and requests a snapshot.
//! - Liveness (`Online`/`Stale`/`Disconnected`) is separate from pane state: losing a node
//!   never rewrites its panes. Nodes are never removed by a dropped connection.
//! - Control requests go only to a currently connected node and fail immediately otherwise;
//!   they are never queued.
//! - Forgetting a node is an explicit operator action ([`OrchestratorCore::forget_node`]), only
//!   for a node with no live connection, and independent of trust (revocation).
//! - A node's identity is its `HostId`, the stable `NodeId`; display names are mutable
//!   presentation and never used for routing.

mod config;
mod conn_state;
mod effects;
mod fleet_hub;
mod forget;
mod ids;
mod liveness;
mod node_conn;
mod node_entry;
mod node_events;
mod node_hello;
mod node_image;
mod orchestrator_core;
mod pending;
mod routing;
mod tick;
mod ui_events;

pub use config::OrchestratorConfig;
pub use effects::Effects;
pub use forget::ForgetError;
pub use ids::{ConnId, UiId};
pub use liveness::Liveness;
pub use orchestrator_core::OrchestratorCore;
