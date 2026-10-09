// SPDX-License-Identifier: MIT

use flight_proto::{
    delta_change, fleet_change::Change, Delta, NodeSavedWorkspaces, NodeServerStatus, PaneRefMsg,
    PaneState, Reject, SavedWorkspace, ServerStatus, Snapshot, Validate,
};
use flight_state::{HostId, PaneRef};
use std::collections::BTreeMap;

/// The authoritative last-known state of one node: what its replication stream has
/// established so far. Only ever changed whole-snapshot or by an accepted, validated delta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NodeImage {
    pub(crate) servers: BTreeMap<String, ServerStatus>,
    pub(crate) panes: BTreeMap<PaneRef, PaneState>,
    /// The node's saved workspaces, replaced whole by each report.
    pub(crate) saved: Vec<SavedWorkspace>,
}

fn pane_key(node: &HostId, pane: &PaneState) -> Result<PaneRef, Reject> {
    let msg = pane
        .pane_ref
        .as_ref()
        .ok_or(Reject::Missing("pane_state.pane_ref"))?;
    let key = PaneRef::try_from(msg)?;
    if key.host != *node {
        return Err(Reject::Mismatch("pane_ref.host"));
    }
    Ok(key)
}

impl NodeImage {
    /// A snapshot as an image, or why it is unacceptable. Nothing is mutated.
    pub(crate) fn from_snapshot(node: &HostId, snapshot: &Snapshot) -> Result<Self, Reject> {
        let mut image = Self::default();
        for pane in &snapshot.panes {
            image.panes.insert(pane_key(node, pane)?, pane.clone());
        }
        for status in &snapshot.servers {
            image.servers.insert(status.server.clone(), status.clone());
        }
        image.saved = snapshot.saved.clone();
        Ok(image)
    }

    /// Become `new`. Returns the fleet changes that describe the difference, and whether the
    /// difference cannot be expressed as changes (a server vanished), in which case the
    /// caller must republish the whole node entry.
    pub(crate) fn replace(&mut self, node: &HostId, new: NodeImage) -> (Vec<Change>, bool) {
        let mut changes = Vec::new();
        for (server, status) in &new.servers {
            if self.servers.get(server) != Some(status) {
                changes.push(server_status(node, status));
            }
        }
        let removed: Vec<&PaneRef> = self
            .panes
            .keys()
            .filter(|k| !new.panes.contains_key(*k))
            .collect();
        changes.extend(
            removed
                .into_iter()
                .map(|k| Change::PaneRemoved(PaneRefMsg::from(k))),
        );
        for (key, pane) in &new.panes {
            if self.panes.get(key) != Some(pane) {
                changes.push(Change::PaneUpsert(pane.clone()));
            }
        }
        if self.saved != new.saved {
            changes.push(saved_change(node, &new.saved));
        }
        let server_lost = self.servers.keys().any(|s| !new.servers.contains_key(s));
        *self = new;
        (changes, server_lost)
    }

    /// Apply one delta atomically: it is fully validated before the image is touched.
    pub(crate) fn apply(&mut self, node: &HostId, delta: &Delta) -> Result<Vec<Change>, Reject> {
        delta.validate()?;
        let change = delta
            .change
            .as_ref()
            .ok_or(Reject::Missing("delta.change"))?;
        Ok(match change {
            delta_change::Change::PaneUpsert(p) => {
                let key = pane_key(node, p)?;
                if self.panes.get(&key) == Some(p) {
                    return Ok(Vec::new());
                }
                self.panes.insert(key, p.clone());
                vec![Change::PaneUpsert(p.clone())]
            }
            delta_change::Change::PaneRemoved(r) => {
                let key = PaneRef::try_from(r)?;
                if key.host != *node {
                    return Err(Reject::Mismatch("pane_ref.host"));
                }
                if self.panes.remove(&key).is_none() {
                    return Ok(Vec::new());
                }
                vec![Change::PaneRemoved(r.clone())]
            }
            delta_change::Change::Saved(s) => {
                if self.saved == s.items {
                    return Ok(Vec::new());
                }
                self.saved = s.items.clone();
                vec![saved_change(node, &s.items)]
            }
            delta_change::Change::ServerStatus(s) => {
                if self.servers.get(&s.server) == Some(s) {
                    return Ok(Vec::new());
                }
                self.servers.insert(s.server.clone(), s.clone());
                vec![server_status(node, s)]
            }
        })
    }
}

fn server_status(node: &HostId, status: &ServerStatus) -> Change {
    Change::ServerStatus(NodeServerStatus {
        node_id: node.as_str().to_owned(),
        status: Some(status.clone()),
    })
}

fn saved_change(node: &HostId, items: &[SavedWorkspace]) -> Change {
    Change::NodeSaved(NodeSavedWorkspaces {
        node_id: node.as_str().to_owned(),
        items: items.to_vec(),
    })
}
