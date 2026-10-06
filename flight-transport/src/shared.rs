// SPDX-License-Identifier: MIT

use flight_orchestrator::{ConnId, Effects, OrchestratorCore, UiId};
use flight_proto::{NodeFrame, OrchestratorFrame, UiEvent};
use flight_state::HostId;
use flight_trust::{EnrollmentTokens, Fingerprint, Role, TrustStore};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Everything the server shares between its connection tasks. Live state only (plus the
/// operator's trust store); held behind one lock and never across an `await`.
pub(crate) struct Shared {
    pub(crate) core: OrchestratorCore,
    pub(crate) trust: TrustStore,
    pub(crate) trust_path: Option<PathBuf>,
    pub(crate) tokens: EnrollmentTokens,
    pub(crate) orchestrator_id: Fingerprint,
    node_tx: HashMap<ConnId, UnboundedSender<OrchestratorFrame>>,
    node_peer: HashMap<ConnId, Fingerprint>,
    ui_tx: HashMap<UiId, UnboundedSender<UiEvent>>,
    ui_peer: HashMap<UiId, Fingerprint>,
    next_id: u64,
}

pub(crate) type SharedState = Arc<Mutex<Shared>>;

pub(crate) fn lock(state: &SharedState) -> MutexGuard<'_, Shared> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    pub(crate) fn new(
        core: OrchestratorCore,
        trust: TrustStore,
        trust_path: Option<PathBuf>,
        orchestrator_id: Fingerprint,
    ) -> Self {
        Self {
            core,
            trust,
            trust_path,
            tokens: EnrollmentTokens::new(),
            orchestrator_id,
            node_tx: HashMap::new(),
            node_peer: HashMap::new(),
            ui_tx: HashMap::new(),
            ui_peer: HashMap::new(),
            next_id: 0,
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    /// Register a node connection authenticated as `peer`.
    pub(crate) fn open_node(
        &mut self,
        peer: Fingerprint,
    ) -> (ConnId, UnboundedReceiver<OrchestratorFrame>) {
        let conn = ConnId(self.id());
        let (tx, rx) = unbounded_channel();
        self.core.node_connected(conn, HostId::new(peer.as_str()));
        self.node_tx.insert(conn, tx);
        self.node_peer.insert(conn, peer);
        (conn, rx)
    }

    pub(crate) fn node_open(&self, conn: ConnId) -> bool {
        self.node_tx.contains_key(&conn)
    }

    pub(crate) fn close_node(&mut self, conn: ConnId) {
        let fx = self.core.node_disconnected(conn);
        self.node_tx.remove(&conn);
        self.node_peer.remove(&conn);
        self.dispatch(fx);
    }

    pub(crate) fn open_ui(&mut self, peer: Fingerprint) -> (UiId, UnboundedReceiver<UiEvent>) {
        let ui = UiId(self.id());
        let (tx, rx) = unbounded_channel();
        self.ui_tx.insert(ui, tx);
        self.ui_peer.insert(ui, peer);
        (ui, rx)
    }

    pub(crate) fn ui_open(&self, ui: UiId) -> bool {
        self.ui_tx.contains_key(&ui)
    }

    pub(crate) fn close_ui(&mut self, ui: UiId) {
        self.core.ui_disconnected(ui);
        self.ui_tx.remove(&ui);
        self.ui_peer.remove(&ui);
    }

    /// Carry out what the orchestrator core asked for. Sends never block (unbounded
    /// channels); a stuck peer is dropped by the core's heartbeat timeout.
    pub(crate) fn dispatch(&mut self, fx: Effects) {
        for (conn, frame) in fx.to_nodes {
            if let Some(tx) = self.node_tx.get(&conn) {
                let _ = tx.send(frame);
            }
        }
        for (ui, event) in fx.to_ui {
            if let Some(tx) = self.ui_tx.get(&ui) {
                let _ = tx.send(event);
            }
        }
        for (conn, _reason) in fx.close {
            self.node_tx.remove(&conn);
            self.node_peer.remove(&conn);
        }
    }

    pub(crate) fn on_node_frame(&mut self, conn: ConnId, frame: NodeFrame) {
        let fx = self.core.on_node_frame(conn, frame, now());
        self.dispatch(fx);
    }

    /// Drop every connection whose identity is no longer authorized (revocation).
    pub(crate) fn enforce_trust(&mut self) {
        let revoked_nodes: Vec<ConnId> = self
            .node_peer
            .iter()
            .filter(|(_, p)| !self.trust.is_authorized(p, Role::Node))
            .map(|(c, _)| *c)
            .collect();
        for conn in revoked_nodes {
            self.close_node(conn);
        }
        let revoked_uis: Vec<UiId> = self
            .ui_peer
            .iter()
            .filter(|(_, p)| !self.trust.is_authorized(p, Role::Ui))
            .map(|(u, _)| *u)
            .collect();
        for ui in revoked_uis {
            self.close_ui(ui);
        }
    }

    /// End every stream (used at shutdown, so graceful close cannot wait on idle peers).
    pub(crate) fn close_all(&mut self) {
        let conns: Vec<ConnId> = self.node_tx.keys().copied().collect();
        for conn in conns {
            self.close_node(conn);
        }
        let uis: Vec<UiId> = self.ui_tx.keys().copied().collect();
        for ui in uis {
            self.close_ui(ui);
        }
    }

    pub(crate) fn tick(&mut self) {
        self.enforce_trust();
        let fx = self.core.tick(now());
        self.dispatch(fx);
    }
}
