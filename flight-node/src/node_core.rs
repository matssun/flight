// SPDX-License-Identifier: MIT

use crate::entry::Entry;
use crate::pane_resolve::{pane_state, resolve_observation};
use crate::unavailable::available;
use crate::{Round, ServerOutcome};
use flight_classify::{AgentKind, Manifest};
use flight_proto::{
    delta_change::Change, Delta, Incarnation, PaneRefMsg, SavedWorkspace, SavedWorkspaces,
    ServerStatus, Snapshot,
};
use flight_state::{HostId, PaneRef, ServerId};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The node's authoritative current state and the replication of it.
///
/// Holds state, not history: each [`Delta`] is derived by comparing the state before and
/// after a round, and means "apply this and you hold what the node holds". A [`Snapshot`]
/// is always sufficient to reconstruct everything externally visible.
pub struct NodeCore {
    host: HostId,
    incarnation: Incarnation,
    next_sequence: u64,
    entries: BTreeMap<PaneRef, Entry>,
    servers: BTreeMap<ServerId, ServerStatus>,
    /// The workspaces this node has saved, as last reported (ADR-008).
    saved: Vec<SavedWorkspace>,
    /// Compiled once: building a manifest compiles its regexes.
    manifests: HashMap<AgentKind, Manifest>,
}

impl NodeCore {
    /// `host` is this node's identity (the `host` of every `PaneRef` it publishes);
    /// `incarnation` is fresh per process (see [`crate::fresh_incarnation`]).
    pub fn new(host: HostId, incarnation: Incarnation) -> Self {
        Self {
            host,
            incarnation,
            next_sequence: 1,
            entries: BTreeMap::new(),
            servers: BTreeMap::new(),
            saved: Vec::new(),
            manifests: [
                AgentKind::Claude,
                AgentKind::Codex,
                AgentKind::OpenCode,
                AgentKind::Pi,
                AgentKind::Other,
            ]
            .into_iter()
            .filter_map(|k| Manifest::builtin(k).ok().map(|m| (k, m)))
            .collect(),
        }
    }

    pub fn host(&self) -> &HostId {
        &self.host
    }

    pub fn incarnation(&self) -> Incarnation {
        self.incarnation
    }

    /// The pid of the process published for `pane`: its incarnation. `None` if the pane is
    /// not published.
    pub fn pane_pid(&self, pane: &PaneRef) -> Option<u32> {
        self.entries.get(pane).map(|e| e.pid)
    }

    /// Whether `pane` is an agent pane this node currently publishes.
    pub fn knows(&self, pane: &PaneRef) -> bool {
        self.entries.contains_key(pane)
    }

    /// The complete current state. Restarts the delta sequence: deltas emitted after this
    /// call continue from `Snapshot(incarnation)` as `sequence = 1, 2, ...`.
    pub fn snapshot(&mut self) -> Snapshot {
        self.next_sequence = 1;
        self.state()
    }

    /// The same complete state without touching the delta sequence: for comparing against a
    /// receiver that has been following the deltas.
    pub fn state(&self) -> Snapshot {
        Snapshot {
            incarnation: self.incarnation.as_bytes().to_vec(),
            panes: self.entries.values().map(|e| e.state.clone()).collect(),
            servers: self.servers.values().cloned().collect(),
            saved: self.saved.clone(),
        }
    }

    /// Fold one observation round into the state; returns the deltas that describe the change.
    pub fn apply(&mut self, round: Round) -> Vec<Delta> {
        let mut changes = Vec::new();
        match round.outcome {
            ServerOutcome::Observed(panes) => {
                self.set_status(&round.server, available(&round.server), &mut changes);
                let mut seen = BTreeSet::new();
                for obs in &panes {
                    let pane_ref = PaneRef {
                        host: self.host.clone(),
                        server: round.server.clone(),
                        pane: obs.pane.clone(),
                    };
                    seen.insert(pane_ref.clone());
                    // The same pane id under another pid is another pane.
                    let before = self.entries.get(&pane_ref).filter(|e| e.pid == obs.pid);
                    let resolved = resolve_observation(
                        obs,
                        self.manifests.get(&obs.agent),
                        before.map(|e| &e.resolved),
                        round.now,
                    );
                    let state = pane_state(
                        &pane_ref,
                        obs,
                        &resolved,
                        before.map(|e| &e.state),
                        round.now,
                    );
                    let changed = self
                        .entries
                        .get(&pane_ref)
                        .is_none_or(|e| e.pid != obs.pid || e.state != state);
                    if changed {
                        changes.push(Change::PaneUpsert(state.clone()));
                    }
                    self.entries.insert(
                        pane_ref,
                        Entry {
                            state,
                            resolved,
                            pid: obs.pid,
                        },
                    );
                }
                self.remove_unseen(&round.server, &seen, &mut changes);
            }
            ServerOutcome::Unavailable(why) => {
                self.set_status(&round.server, why.status(&round.server), &mut changes);
                if why.panes_are_gone() {
                    self.remove_unseen(&round.server, &BTreeSet::new(), &mut changes);
                }
            }
        }
        self.sequence(changes)
    }

    /// Replace the saved-workspace list. Returns the delta that says so, if the list changed
    /// and `announce` is set. With `announce` unset (the orchestrator did not accept the
    /// capability) the state is still updated, so a later snapshot has it, but no sequence
    /// number is consumed: a delta that is never sent would look like a gap.
    pub fn set_saved(&mut self, saved: Vec<SavedWorkspace>, announce: bool) -> Vec<Delta> {
        if self.saved == saved {
            return Vec::new();
        }
        self.saved = saved;
        if !announce {
            return Vec::new();
        }
        self.sequence(vec![Change::Saved(SavedWorkspaces {
            items: self.saved.clone(),
        })])
    }

    fn set_status(&mut self, server: &ServerId, status: ServerStatus, out: &mut Vec<Change>) {
        if self.servers.get(server) != Some(&status) {
            self.servers.insert(server.clone(), status.clone());
            out.push(Change::ServerStatus(status));
        }
    }

    /// Drop every pane of `server` not in `seen`, with its Tracking.
    fn remove_unseen(
        &mut self,
        server: &ServerId,
        seen: &BTreeSet<PaneRef>,
        out: &mut Vec<Change>,
    ) {
        let gone: Vec<PaneRef> = self
            .entries
            .keys()
            .filter(|k| &k.server == server && !seen.contains(*k))
            .cloned()
            .collect();
        for pane_ref in gone {
            self.entries.remove(&pane_ref);
            out.push(Change::PaneRemoved(PaneRefMsg::from(&pane_ref)));
        }
    }

    fn sequence(&mut self, changes: Vec<Change>) -> Vec<Delta> {
        changes
            .into_iter()
            .map(|change| {
                let sequence = self.next_sequence;
                self.next_sequence = self.next_sequence.saturating_add(1);
                Delta {
                    incarnation: self.incarnation.as_bytes().to_vec(),
                    sequence,
                    change: Some(change),
                }
            })
            .collect()
    }
}
