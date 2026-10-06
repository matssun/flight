// SPDX-License-Identifier: MIT

use flight_proto::{fleet_change::Change, NodeStatusChanged, NodeStatusCode};
use flight_state::HostId;

/// Whether the orchestrator can currently hear a node. Says nothing about pane state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Connected,
    /// Connected, but heartbeats have been missed.
    Stale,
    Disconnected,
}

impl Liveness {
    pub(crate) fn code(self) -> NodeStatusCode {
        match self {
            Self::Connected => NodeStatusCode::Online,
            Self::Stale => NodeStatusCode::Stale,
            Self::Disconnected => NodeStatusCode::Disconnected,
        }
    }
}

/// The fleet change that reports a node's liveness.
pub(crate) fn status_change(node: &HostId, liveness: Liveness) -> Change {
    Change::NodeStatus(NodeStatusChanged {
        node_id: node.as_str().to_owned(),
        status: liveness.code() as i32,
    })
}
