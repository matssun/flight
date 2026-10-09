// SPDX-License-Identifier: MIT

use crate::node_link::LinkLog;
use crate::outbox::{Outbox, PushError};
use crate::terminal_relay::{Ends, Relay};
use flight_orchestrator::{ConnId, Effects, OrchestratorCore, Side, TerminalId, UiId};
use flight_proto::{
    orchestrator_body, ui_event_body, ExitReasonCode, NodeFrame, OrchestratorFrame, UiEvent,
};
use flight_state::HostId;
use flight_trust::{EnrollmentTokens, Fingerprint, Role, TrustStore};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Frames or events queued per peer before the replication backlog is discarded and a
/// snapshot is sent instead (or, for reliable traffic, the peer is dropped).
pub(crate) const OUTBOX_CAPACITY: usize = 256;

/// Outbox classes for orchestrator -> node frames.
const HEARTBEAT: u8 = 0;
const RESYNC: u8 = 1;

/// A terminal side that cannot move a frame for this long ends the terminal.
pub(crate) const DEFAULT_TERMINAL_STALL: std::time::Duration = std::time::Duration::from_secs(30);

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
    node_out: HashMap<ConnId, Arc<Outbox<OrchestratorFrame>>>,
    node_peer: HashMap<ConnId, Fingerprint>,
    ui_out: HashMap<UiId, Arc<Outbox<UiEvent>>>,
    ui_peer: HashMap<UiId, Fingerprint>,
    next_id: u64,
    log: Option<LinkLog>,
    relays: HashMap<TerminalId, Relay>,
    terminal_stall: std::time::Duration,
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
            node_out: HashMap::new(),
            node_peer: HashMap::new(),
            ui_out: HashMap::new(),
            ui_peer: HashMap::new(),
            next_id: 0,
            log: None,
            relays: HashMap::new(),
            terminal_stall: DEFAULT_TERMINAL_STALL,
        }
    }

    pub(crate) fn set_log(&mut self, log: LinkLog) {
        self.log = Some(log);
    }

    fn say(&self, line: String) {
        if let Some(log) = &self.log {
            log(line);
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
    ) -> (ConnId, Arc<Outbox<OrchestratorFrame>>) {
        let conn = ConnId(self.id());
        let outbox = Arc::new(Outbox::new(OUTBOX_CAPACITY));
        self.core.node_connected(conn, HostId::new(peer.as_str()));
        self.node_out.insert(conn, outbox.clone());
        self.node_peer.insert(conn, peer);
        (conn, outbox)
    }

    pub(crate) fn node_open(&self, conn: ConnId) -> bool {
        self.node_out.contains_key(&conn)
    }

    pub(crate) fn close_node(&mut self, conn: ConnId) {
        let fx = self.core.node_disconnected(conn);
        if let Some(out) = self.node_out.remove(&conn) {
            out.close();
        }
        self.node_peer.remove(&conn);
        self.dispatch(fx);
    }

    pub(crate) fn open_ui(&mut self, peer: Fingerprint) -> (UiId, Arc<Outbox<UiEvent>>) {
        let ui = UiId(self.id());
        let outbox = Arc::new(Outbox::new(OUTBOX_CAPACITY));
        self.core.ui_identified(ui, peer.as_str());
        self.ui_out.insert(ui, outbox.clone());
        self.ui_peer.insert(ui, peer);
        (ui, outbox)
    }

    pub(crate) fn ui_open(&self, ui: UiId) -> bool {
        self.ui_out.contains_key(&ui)
    }

    pub(crate) fn close_ui(&mut self, ui: UiId) {
        self.core.ui_disconnected(ui);
        if let Some(out) = self.ui_out.remove(&ui) {
            out.close();
        }
        self.ui_peer.remove(&ui);
    }

    /// A UI fell behind and its queued deltas were discarded: give it a fresh snapshot (which
    /// restarts its delta sequence) and mark it in sync, atomically with respect to new deltas.
    pub(crate) fn resync_ui(&mut self, ui: UiId) {
        let Some(out) = self.ui_out.get(&ui).cloned() else {
            return;
        };
        let fx = self.core.subscribe(ui);
        for (_, event) in fx.to_ui {
            if out.push_reliable(event).is_err() {
                self.close_ui(ui);
                return;
            }
        }
        out.clear_overflow();
    }

    /// Carry out what the orchestrator core asked for. Nothing blocks and nothing grows
    /// without bound: replication deltas are disposable (an overflowing peer is resynced),
    /// heartbeats coalesce, and a peer that will not read reliable traffic is dropped.
    pub(crate) fn dispatch(&mut self, fx: Effects) {
        for note in fx.notes {
            self.say(note);
        }
        for (id, reason) in &fx.terminals_ended {
            if let Some(relay) = self.relays.remove(id) {
                relay.abort(*reason);
                self.say(format!(
                    "terminal closed: {reason:?} after {}s",
                    relay.started.elapsed().as_secs()
                ));
            }
        }
        let mut slow_nodes = Vec::new();
        let mut slow_uis = Vec::new();
        for (conn, frame) in fx.to_nodes {
            let Some(out) = self.node_out.get(&conn) else {
                continue;
            };
            let pushed = match &frame.body {
                Some(orchestrator_body::Body::Heartbeat(_)) => out.push_coalesced(HEARTBEAT, frame),
                Some(orchestrator_body::Body::Resync(_)) => out.push_coalesced(RESYNC, frame),
                _ => out.push_reliable(frame),
            };
            if pushed == Err(PushError::Full) {
                slow_nodes.push(conn);
            }
        }
        for (ui, event) in fx.to_ui {
            let Some(out) = self.ui_out.get(&ui) else {
                continue;
            };
            let pushed = match &event.body {
                Some(ui_event_body::Body::Delta(_)) => out.push_delta(event),
                _ => out.push_reliable(event),
            };
            if pushed == Err(PushError::Full) {
                slow_uis.push(ui);
            }
        }
        for (conn, reason) in fx.close {
            self.say(format!("closing node connection: {reason}"));
            if let Some(out) = self.node_out.remove(&conn) {
                out.close();
            }
            self.node_peer.remove(&conn);
        }
        for conn in slow_nodes {
            self.close_node(conn);
        }
        for ui in slow_uis {
            self.close_ui(ui);
        }
    }

    pub(crate) fn on_node_frame(&mut self, conn: ConnId, frame: NodeFrame) {
        let fx = self.core.on_node_frame(conn, frame, now());
        self.dispatch(fx);
    }

    pub(crate) fn terminal_stall(&self) -> std::time::Duration {
        self.terminal_stall
    }

    pub(crate) fn set_terminal_stall(&mut self, stall: std::time::Duration) {
        self.terminal_stall = stall;
    }

    /// The largest number of frames ever queued in any terminal direction.
    pub(crate) fn terminal_queue_peak(&self) -> usize {
        self.relays.values().map(Relay::peak).max().unwrap_or(0)
    }

    /// Relays still held (queues and abort signal), one per terminal not yet finished.
    pub(crate) fn terminal_relays(&self) -> usize {
        self.relays.len()
    }

    pub(crate) fn terminals_open(&self) -> usize {
        self.core.terminal_count()
    }

    /// A terminal stream arrived: the core decides whether it may attach, and the relay hands
    /// it its half.
    pub(crate) fn attach_terminal(
        &mut self,
        side: Side,
        id: &[u8],
        identity: &str,
    ) -> Option<(TerminalId, Ends)> {
        let (id, _) = self.core.terminal_attach(side, id, identity).ok()?;
        let ends = self
            .relays
            .entry(id)
            .or_insert_with(Relay::new)
            .claim(side)?;
        Some((id, ends))
    }

    /// The UI's side of a terminal is over; the node's is still finishing.
    pub(crate) fn terminal_closing(&mut self, id: &TerminalId) {
        self.core.terminal_closing(id);
    }

    /// One side's stream ended: the id is dead and the relay is gone.
    pub(crate) fn finish_terminal(&mut self, id: TerminalId, side: Side, reason: ExitReasonCode) {
        self.core.terminal_ended(&id);
        if let Some(relay) = self.relays.remove(&id) {
            relay.abort(reason);
            self.say(format!(
                "terminal closed: {reason:?} (ended by the {side:?} side) after {}s",
                relay.started.elapsed().as_secs()
            ));
        }
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
            let fx = self.core.end_node_terminals(conn, ExitReasonCode::Revoked);
            self.dispatch(fx);
            self.close_node(conn);
        }
        let revoked_uis: Vec<UiId> = self
            .ui_peer
            .iter()
            .filter(|(_, p)| !self.trust.is_authorized(p, Role::Ui))
            .map(|(u, _)| *u)
            .collect();
        for ui in revoked_uis {
            if let Some(identity) = self.ui_peer.get(&ui).cloned() {
                let fx = self
                    .core
                    .end_ui_terminals(identity.as_str(), ExitReasonCode::Revoked);
                self.dispatch(fx);
            }
            self.close_ui(ui);
        }
    }

    /// The longest outbound queue across all peers. Bounded by [`OUTBOX_CAPACITY`] by design.
    pub(crate) fn max_backlog(&self) -> usize {
        let nodes = self.node_out.values().map(|o| o.len());
        let uis = self.ui_out.values().map(|o| o.len());
        nodes.chain(uis).max().unwrap_or(0)
    }

    /// End every stream (used at shutdown, so graceful close cannot wait on idle peers).
    pub(crate) fn close_all(&mut self) {
        let fx = self.core.end_all_terminals(ExitReasonCode::Shutdown);
        self.dispatch(fx);
        let conns: Vec<ConnId> = self.node_out.keys().copied().collect();
        for conn in conns {
            self.close_node(conn);
        }
        let uis: Vec<UiId> = self.ui_out.keys().copied().collect();
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
