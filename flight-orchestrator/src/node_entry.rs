// SPDX-License-Identifier: MIT

use crate::node_image::NodeImage;
use crate::{ConnId, Liveness};
use flight_proto::{NodeView, ReplicationCursor};
use flight_state::HostId;

/// Everything the orchestrator holds for one node. All of it is live state that a
/// reconnecting node rebuilds; none of it is durable.
#[derive(Debug)]
pub(crate) struct NodeEntry {
    pub(crate) display_name: String,
    /// Capabilities both sides agreed on.
    pub(crate) accepted: Vec<String>,
    pub(crate) image: NodeImage,
    pub(crate) liveness: Liveness,
    pub(crate) conn: Option<ConnId>,
    pub(crate) cursor: ReplicationCursor,
    /// A `ResyncRequest` is outstanding; do not repeat it until a snapshot arrives.
    pub(crate) resync_requested: bool,
    pub(crate) last_seen: u64,
    pub(crate) last_ping: u64,
}

impl NodeEntry {
    pub(crate) fn new(display_name: String, accepted: Vec<String>, conn: ConnId, now: u64) -> Self {
        Self {
            display_name,
            accepted,
            image: NodeImage::default(),
            liveness: Liveness::Connected,
            conn: Some(conn),
            cursor: ReplicationCursor::new(),
            resync_requested: false,
            last_seen: now,
            last_ping: now,
        }
    }

    pub(crate) fn view(&self, node: &HostId) -> NodeView {
        NodeView {
            node_id: node.as_str().to_owned(),
            display_name: self.display_name.clone(),
            status: self.liveness.code() as i32,
            servers: self.image.servers.values().cloned().collect(),
            panes: self.image.panes.values().cloned().collect(),
        }
    }
}
