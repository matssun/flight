// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use flight_classify::AgentKind;
use flight_node::{Control, ControlError};
use flight_node::{NodeCore, NodeSession, PaneObservation, Round, ServerOutcome, Unavailable};
use flight_orchestrator::{ConnId, Effects, OrchestratorConfig, OrchestratorCore, UiId};
use flight_proto::{
    fleet_change::Change, node_body, orchestrator_body, ui_event_body, FleetSnapshot, Heartbeat,
    Incarnation, NodeFrame, NodeView, OrchestratorFrame, PaneState, ReplicationCursor,
    ServerStatus, Step, UiEvent,
};
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use std::collections::BTreeMap;

pub const PERMIT_SCREEN: &str =
    include_str!("../../../flight-classify/tests/fixtures/claude-permit.txt");
pub const IDLE_SCREEN: &str = "Done!\n\n❯\n";
pub const BUSY_SCREEN: &str = "✻ Trapping Gollum… (8s · ↑ 240 tokens)\n\n❯\n";

pub fn inc(n: u8) -> Incarnation {
    Incarnation::from_bytes([n; Incarnation::LEN])
}

pub fn server() -> ServerId {
    ServerId::new("flight")
}

pub fn obs(pane: &str, pid: u32, screen: &str) -> PaneObservation {
    PaneObservation {
        pane: PaneId::new(pane),
        pid,
        agent: AgentKind::Claude,
        session: "work".into(),
        window: "agent".into(),
        path: "/tmp".into(),
        command: "claude".into(),
        title: String::new(),
        focused: false,
        screen_lines: screen.lines().map(str::to_owned).collect(),
    }
}

pub fn round(now: u64, panes: Vec<PaneObservation>) -> Round {
    Round {
        server: server(),
        now,
        outcome: ServerOutcome::Observed(panes),
    }
}

pub fn down(now: u64, why: Unavailable) -> Round {
    Round {
        server: server(),
        now,
        outcome: ServerOutcome::Unavailable(why),
    }
}

pub struct NoControl;

impl Control for NoControl {
    fn capture(&self, _: &ServerId, _: &PaneId, _: u32) -> Result<String, ControlError> {
        Ok(String::new())
    }
    fn kill_pane(&self, _: &ServerId, _: &PaneId) -> Result<(), ControlError> {
        Ok(())
    }
    fn create_session(&self, _: &ServerId, _: &str, _: &str, _: &str) -> Result<(), ControlError> {
        Ok(())
    }
}

/// A simulated node: a real `NodeSession` (so frames are exactly what a node sends).
pub struct SimNode {
    pub id: HostId,
    pub session: NodeSession<NoControl>,
    pub conn: Option<ConnId>,
    pub incarnation: u8,
}

impl SimNode {
    pub fn new(id: &str, name: &str, incarnation: u8) -> Self {
        Self {
            id: HostId::new(id),
            session: NodeSession::new(
                NodeCore::new(HostId::new(id), inc(incarnation)),
                NoControl,
                name,
            ),
            conn: None,
            incarnation,
        }
    }

    pub fn restart(&mut self, name: &str) {
        self.incarnation = self.incarnation.wrapping_add(1).max(1);
        self.session = NodeSession::new(
            NodeCore::new(self.id.clone(), inc(self.incarnation)),
            NoControl,
            name,
        );
    }
}

/// The UI-side oracle: applies a FleetSnapshot and FleetDeltas exactly as a UI would.
#[derive(Default)]
pub struct FleetMirror {
    cursor: ReplicationCursor,
    nodes: BTreeMap<String, MirrorNode>,
}

#[derive(Default, Clone)]
struct MirrorNode {
    name: String,
    status: i32,
    servers: BTreeMap<String, ServerStatus>,
    panes: BTreeMap<PaneRef, PaneState>,
}

fn key(p: &PaneState) -> PaneRef {
    PaneRef::try_from(p.pane_ref.as_ref().expect("pane_ref")).expect("ref")
}

impl FleetMirror {
    pub fn in_sync(&self) -> bool {
        self.cursor.in_sync()
    }

    pub fn apply(&mut self, event: &UiEvent) -> Step {
        match event.body.as_ref().expect("body") {
            ui_event_body::Body::Snapshot(s) => {
                self.cursor.on_snapshot(s.incarnation().expect("inc"));
                self.nodes = s
                    .nodes
                    .iter()
                    .map(|n| (n.node_id.clone(), load(n)))
                    .collect();
                Step::Apply
            }
            ui_event_body::Body::Delta(d) => {
                let step = self
                    .cursor
                    .on_delta(d.incarnation().expect("inc"), d.sequence);
                if step == Step::Apply {
                    self.change(d.change.as_ref().expect("change"));
                }
                step
            }
            ui_event_body::Body::Response(_) => Step::Apply,
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
                let host = p.pane_ref.as_ref().expect("ref").host.clone();
                if let Some(n) = self.nodes.get_mut(&host) {
                    n.panes.insert(key(p), p.clone());
                }
            }
            Change::PaneRemoved(r) => {
                if let Some(n) = self.nodes.get_mut(&r.host) {
                    n.panes.remove(&PaneRef::try_from(r).expect("ref"));
                }
            }
            Change::ServerStatus(s) => {
                let status = s.status.clone().expect("status");
                if let Some(n) = self.nodes.get_mut(&s.node_id) {
                    n.servers.insert(status.server.clone(), status);
                }
            }
            Change::NodeRemoved(n) => {
                self.nodes.remove(&n.node_id);
            }
        }
    }

    /// The mirror as the same shape the orchestrator publishes.
    pub fn views(&self) -> Vec<NodeView> {
        self.nodes
            .iter()
            .map(|(id, n)| NodeView {
                node_id: id.clone(),
                display_name: n.name.clone(),
                status: n.status,
                servers: n.servers.values().cloned().collect(),
                panes: n.panes.values().cloned().collect(),
            })
            .collect()
    }
}

fn load(v: &NodeView) -> MirrorNode {
    MirrorNode {
        name: v.display_name.clone(),
        status: v.status,
        servers: v
            .servers
            .iter()
            .map(|s| (s.server.clone(), s.clone()))
            .collect(),
        panes: v.panes.iter().map(|p| (key(p), p.clone())).collect(),
    }
}

/// Orchestrator + simulated nodes + UI mirrors, wired by delivering Effects.
pub struct World {
    pub orch: OrchestratorCore,
    pub nodes: Vec<SimNode>,
    pub uis: BTreeMap<UiId, FleetMirror>,
    pub now: u64,
    next_conn: u64,
    orch_incarnation: u8,
    /// Requests the orchestrator forwarded to nodes: (node id, frame).
    pub forwarded: Vec<(String, OrchestratorFrame)>,
    /// Frames from nodes are dropped while set (to simulate loss without a disconnect).
    pub drop_node_frames: bool,
    pub closed: Vec<(ConnId, String)>,
    /// Muted nodes are silent: no heartbeats, no echoes.
    pub muted: std::collections::BTreeSet<usize>,
    pub responses: Vec<(UiId, UiEvent)>,
}

impl World {
    pub fn new(nodes: Vec<SimNode>) -> Self {
        Self {
            orch: OrchestratorCore::new(OrchestratorConfig::default(), inc(200)),
            nodes,
            uis: BTreeMap::new(),
            now: 1_000,
            next_conn: 1,
            orch_incarnation: 200,
            forwarded: Vec::new(),
            drop_node_frames: false,
            closed: Vec::new(),
            muted: Default::default(),
            responses: Vec::new(),
        }
    }

    pub fn mute(&mut self, i: usize) {
        self.muted.insert(i);
    }

    pub fn unmute(&mut self, i: usize) {
        self.muted.remove(&i);
    }

    pub fn restart_orchestrator(&mut self) {
        self.orch_incarnation = self.orch_incarnation.wrapping_add(1).max(1);
        self.orch =
            OrchestratorCore::new(OrchestratorConfig::default(), inc(self.orch_incarnation));
        for n in &mut self.nodes {
            n.conn = None;
        }
        // UIs lose their stream with the orchestrator; they resubscribe by themselves.
        let ids: Vec<UiId> = self.uis.keys().copied().collect();
        for ui in ids {
            self.uis.insert(ui, FleetMirror::default());
            let fx = self.orch.subscribe(ui);
            self.deliver(fx);
        }
    }

    pub fn subscribe(&mut self, ui: UiId) {
        self.uis.entry(ui).or_default();
        let fx = self.orch.subscribe(ui);
        self.deliver(fx);
    }

    /// Open a connection for node `i` and run the handshake.
    pub fn connect(&mut self, i: usize) -> ConnId {
        let conn = ConnId(self.next_conn);
        self.next_conn += 1;
        let hello = {
            let node = &mut self.nodes[i];
            node.conn = Some(conn);
            self.orch.node_connected(conn, node.id.clone());
            node.session.connect(vec![server().to_string()])
        };
        self.send_to_orch(conn, hello);
        conn
    }

    pub fn disconnect(&mut self, i: usize) {
        if let Some(conn) = self.nodes[i].conn.take() {
            let fx = self.orch.node_disconnected(conn);
            self.deliver(fx);
        }
    }

    /// Observe on node `i`; changes flow to the orchestrator if it is connected.
    pub fn observe(&mut self, i: usize, r: Round) {
        let frames = self.nodes[i].session.observe(vec![r]);
        if let Some(conn) = self.nodes[i].conn {
            for f in frames {
                self.send_to_orch(conn, f);
            }
        }
    }

    pub fn send_to_orch(&mut self, conn: ConnId, frame: NodeFrame) {
        if self.drop_node_frames {
            return;
        }
        let fx = self.orch.on_node_frame(conn, frame, self.now);
        self.deliver(fx);
    }

    /// Node heartbeats for every connected node, then the orchestrator's clock.
    pub fn advance(&mut self, secs: u64) {
        self.now += secs;
        {
            for i in 0..self.nodes.len() {
                if self.muted.contains(&i) {
                    continue;
                }
                if let Some(conn) = self.nodes[i].conn {
                    let hb = NodeFrame {
                        body: Some(node_body::Body::Heartbeat(Heartbeat { seq: self.now })),
                    };
                    self.send_to_orch(conn, hb);
                }
            }
        }
        let fx = self.orch.tick(self.now);
        self.deliver(fx);
    }

    pub fn deliver(&mut self, fx: Effects) {
        for (conn, why) in fx.close {
            for n in &mut self.nodes {
                if n.conn == Some(conn) {
                    n.conn = None;
                }
            }
            self.closed.push((conn, why));
        }
        for (ui, event) in fx.to_ui {
            if matches!(event.body, Some(ui_event_body::Body::Response(_))) {
                self.responses.push((ui, event.clone()));
            }
            let resubscribe = self
                .uis
                .get_mut(&ui)
                .is_some_and(|m| m.apply(&event) == Step::Resync);
            if resubscribe {
                let fx = self.orch.subscribe(ui);
                self.deliver(fx);
            }
        }
        for (conn, frame) in fx.to_nodes {
            self.node_receives(conn, frame);
        }
    }

    fn node_receives(&mut self, conn: ConnId, frame: OrchestratorFrame) {
        let Some(i) = self.nodes.iter().position(|n| n.conn == Some(conn)) else {
            return;
        };
        if self.muted.contains(&i) {
            return;
        }
        if matches!(frame.body, Some(orchestrator_body::Body::Request(_))) {
            self.forwarded.push((self.nodes[i].id.to_string(), frame));
            return;
        }
        let out = self.nodes[i].session.on_frame(frame, self.now);
        for f in out.frames {
            self.send_to_orch(conn, f);
        }
        if out.close.is_some() {
            self.nodes[i].conn = None;
        }
    }

    /// Ask every connected, unmuted node for a snapshot, as the orchestrator does on a gap.
    pub fn flush(&mut self) {
        for i in 0..self.nodes.len() {
            if let Some(conn) = self.nodes[i].conn {
                self.node_receives(
                    conn,
                    OrchestratorFrame {
                        body: Some(orchestrator_body::Body::Resync(
                            flight_proto::ResyncRequest {
                                reason: "flush".into(),
                            },
                        )),
                    },
                );
            }
        }
    }

    pub fn views(&self) -> Vec<NodeView> {
        self.orch.fleet_snapshot().nodes
    }

    pub fn snapshot(&self) -> FleetSnapshot {
        self.orch.fleet_snapshot()
    }

    pub fn node_view(&self, id: &str) -> Option<NodeView> {
        self.views().into_iter().find(|v| v.node_id == id)
    }
}

pub fn pane_ids(v: &NodeView) -> Vec<String> {
    v.panes
        .iter()
        .filter_map(|p| p.pane_ref.as_ref().map(|r| r.pane.clone()))
        .collect()
}

pub fn pane_ref(node: &str, pane: &str) -> flight_proto::PaneRefMsg {
    flight_proto::PaneRefMsg {
        host: node.into(),
        server: "flight".into(),
        pane: pane.into(),
    }
}

pub fn simple_world() -> World {
    World::new(vec![
        SimNode::new("node-a", "mini-1", 1),
        SimNode::new("node-b", "mini-2", 1),
    ])
}
