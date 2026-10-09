// SPDX-License-Identifier: MIT

use crate::fleet_change::Change;
use crate::{
    ui_event_body, NodeStatusCode, NodeView, PaneState, ReplicationCursor, SavedWorkspace,
    ServerStatus, Step, UiEvent,
};
use flight_state::PaneRef;
use std::collections::BTreeMap;

/// One node as a UI holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FleetNode {
    pub display_name: String,
    /// A [`NodeStatusCode`] value; liveness only, never pane state.
    pub status: i32,
    pub servers: BTreeMap<String, ServerStatus>,
    pub panes: BTreeMap<PaneRef, PaneState>,
    /// The workspaces the node has saved, as it last reported them.
    pub saved: Vec<SavedWorkspace>,
}

impl FleetNode {
    pub fn status_code(&self) -> Option<NodeStatusCode> {
        NodeStatusCode::try_from(self.status).ok()
    }
}

/// The fleet as a subscriber reconstructs it from a `FleetSnapshot` and the `FleetDelta`s
/// that follow. Deltas apply only in order within one orchestrator incarnation; anything else
/// returns [`Step::Resync`] and leaves the image as it was, so the caller must re-subscribe.
#[derive(Debug, Default, Clone)]
pub struct FleetImage {
    cursor: ReplicationCursor,
    nodes: BTreeMap<String, FleetNode>,
}

fn pane_key(p: &PaneState) -> Option<PaneRef> {
    PaneRef::try_from(p.pane_ref.as_ref()?).ok()
}

fn load(v: &NodeView) -> FleetNode {
    FleetNode {
        display_name: v.display_name.clone(),
        status: v.status,
        servers: v
            .servers
            .iter()
            .map(|s| (s.server.clone(), s.clone()))
            .collect(),
        panes: v
            .panes
            .iter()
            .filter_map(|p| Some((pane_key(p)?, p.clone())))
            .collect(),
        saved: v.saved.clone(),
    }
}

impl FleetImage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a snapshot is held and the delta stream since is unbroken.
    pub fn in_sync(&self) -> bool {
        self.cursor.in_sync()
    }

    /// Forget the stream (the connection dropped). The last-known nodes stay visible.
    pub fn disconnected(&mut self) {
        self.cursor.reset();
    }

    pub fn nodes(&self) -> &BTreeMap<String, FleetNode> {
        &self.nodes
    }

    pub fn apply(&mut self, event: &UiEvent) -> Step {
        match event.body.as_ref() {
            Some(ui_event_body::Body::Snapshot(s)) => {
                let Ok(incarnation) = s.incarnation() else {
                    return Step::Resync;
                };
                self.cursor.on_snapshot(incarnation);
                self.nodes = s
                    .nodes
                    .iter()
                    .map(|n| (n.node_id.clone(), load(n)))
                    .collect();
                Step::Apply
            }
            Some(ui_event_body::Body::Delta(d)) => {
                let Ok(incarnation) = d.incarnation() else {
                    return Step::Resync;
                };
                let step = self.cursor.on_delta(incarnation, d.sequence);
                if step == Step::Apply {
                    if let Some(change) = d.change.as_ref() {
                        self.change(change);
                    }
                }
                step
            }
            _ => Step::Apply,
        }
    }

    fn change(&mut self, change: &Change) {
        match change {
            Change::NodeUpsert(v) => {
                self.nodes.insert(v.node_id.clone(), load(v));
            }
            Change::NodeStatus(s) => {
                if let Some(n) = self.nodes.get_mut(&s.node_id) {
                    n.status = s.status;
                }
            }
            Change::PaneUpsert(p) => {
                if let (Some(key), Some(r)) = (pane_key(p), p.pane_ref.as_ref()) {
                    if let Some(n) = self.nodes.get_mut(&r.host) {
                        n.panes.insert(key, p.clone());
                    }
                }
            }
            Change::PaneRemoved(r) => {
                if let (Ok(key), Some(n)) = (PaneRef::try_from(r), self.nodes.get_mut(&r.host)) {
                    n.panes.remove(&key);
                }
            }
            Change::ServerStatus(s) => {
                if let (Some(status), Some(n)) = (s.status.as_ref(), self.nodes.get_mut(&s.node_id))
                {
                    n.servers.insert(status.server.clone(), status.clone());
                }
            }
            Change::NodeSaved(s) => {
                if let Some(n) = self.nodes.get_mut(&s.node_id) {
                    n.saved = s.items.clone();
                }
            }
            Change::NodeRemoved(n) => {
                self.nodes.remove(&n.node_id);
            }
        }
    }

    /// The image in the shape the orchestrator publishes (for comparison and tests).
    pub fn views(&self) -> Vec<NodeView> {
        self.nodes
            .iter()
            .map(|(id, n)| NodeView {
                node_id: id.clone(),
                display_name: n.display_name.clone(),
                status: n.status,
                servers: n.servers.values().cloned().collect(),
                panes: n.panes.values().cloned().collect(),
                saved: n.saved.clone(),
            })
            .collect()
    }
}
