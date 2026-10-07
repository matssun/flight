// SPDX-License-Identifier: MIT

use crate::{Effects, OrchestratorCore};
use flight_proto::{fleet_change::Change, NodeRemoved};
use flight_state::HostId;
use std::fmt;

/// Why a node could not be forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetError {
    /// The orchestrator has never seen (or already forgot) this node.
    Unknown,
    /// The node still has a live connection. Forgetting it would only make it reappear;
    /// stop it or revoke its trust first.
    StillConnected,
}

impl fmt::Display for ForgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "no such node"),
            Self::StillConnected => write!(
                f,
                "the node is still connected; stop it or revoke it first (forgetting is for nodes that are gone)"
            ),
        }
    }
}

impl std::error::Error for ForgetError {}

impl OrchestratorCore {
    /// Remove a node's last-known image from the fleet and tell every UI. This is the
    /// operator's explicit decision that a disconnected node is no longer wanted on the
    /// dashboard. It is deliberately separate from trust: nothing here changes who may
    /// connect, and a trusted node that connects again simply reappears with a fresh image.
    /// Disconnection alone never removes a node; a powered-off machine stays known.
    pub fn forget_node(&mut self, node: &HostId) -> Result<Effects, ForgetError> {
        let entry = self.nodes.get(node).ok_or(ForgetError::Unknown)?;
        if entry.conn.is_some() {
            return Err(ForgetError::StillConnected);
        }
        self.nodes.remove(node);
        let mut fx = Effects::default();
        self.publish(
            &mut fx,
            vec![Change::NodeRemoved(NodeRemoved {
                node_id: node.as_str().to_owned(),
            })],
        );
        Ok(fx)
    }
}
