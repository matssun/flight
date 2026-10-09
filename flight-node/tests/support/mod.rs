// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use flight_classify::AgentKind;
use flight_node::{NodeCore, PaneObservation, Round, ServerOutcome, Unavailable};
use flight_proto::{
    delta_change::Change, Delta, Incarnation, PaneState, ReplicationCursor, ServerStatus, Snapshot,
    Step,
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

pub fn core() -> NodeCore {
    NodeCore::new(HostId::new("node-1"), inc(1))
}

pub fn server() -> ServerId {
    ServerId::new("flight")
}

pub fn obs(pane: &str, pid: u32, screen: &str, focused: bool) -> PaneObservation {
    PaneObservation {
        pane: PaneId::new(pane),
        pid,
        agent: AgentKind::Claude,
        session: "work".into(),
        window: "agent".into(),
        path: "/tmp".into(),
        command: "claude".into(),
        title: String::new(),
        focused,
        screen_lines: screen.lines().map(str::to_owned).collect(),
        placement: Default::default(),
    }
}

pub fn round(server_id: &ServerId, now: u64, panes: Vec<PaneObservation>) -> Round {
    Round {
        server: server_id.clone(),
        now,
        outcome: ServerOutcome::Observed(panes),
    }
}

pub fn down(server_id: &ServerId, now: u64, why: Unavailable) -> Round {
    Round {
        server: server_id.clone(),
        now,
        outcome: ServerOutcome::Unavailable(why),
    }
}

pub fn pane_ref(server_id: &ServerId, pane: &str) -> PaneRef {
    PaneRef {
        host: HostId::new("node-1"),
        server: server_id.clone(),
        pane: PaneId::new(pane),
    }
}

/// What the orchestrator would hold, built only from a snapshot and the deltas that follow.
/// (Slice 3's orchestrator core will own the real version; this is the test oracle.)
#[derive(Default)]
pub struct Mirror {
    cursor: ReplicationCursor,
    pub panes: BTreeMap<PaneRef, PaneState>,
    pub servers: BTreeMap<String, ServerStatus>,
    pub saved: Vec<flight_proto::SavedWorkspace>,
}

impl Mirror {
    pub fn load(&mut self, snapshot: &Snapshot) {
        self.cursor
            .on_snapshot(snapshot.incarnation().expect("incarnation"));
        self.panes = snapshot.panes.iter().map(|p| (key(p), p.clone())).collect();
        self.servers = snapshot
            .servers
            .iter()
            .map(|s| (s.server.clone(), s.clone()))
            .collect();
        self.saved = snapshot.saved.clone();
    }

    pub fn apply(&mut self, delta: &Delta) -> Step {
        let step = self
            .cursor
            .on_delta(delta.incarnation().expect("incarnation"), delta.sequence);
        if step == Step::Resync {
            return step;
        }
        match delta.change.as_ref().expect("change") {
            Change::PaneUpsert(p) => {
                self.panes.insert(key(p), p.clone());
            }
            Change::PaneRemoved(r) => {
                self.panes.remove(&PaneRef::try_from(r).expect("ref"));
            }
            Change::ServerStatus(s) => {
                self.servers.insert(s.server.clone(), s.clone());
            }
            Change::Saved(s) => {
                self.saved = s.items.clone();
            }
        }
        step
    }

    pub fn matches(&self, snapshot: &Snapshot) -> bool {
        let mut other = Mirror::default();
        other.load(snapshot);
        self.panes == other.panes && self.servers == other.servers && self.saved == other.saved
    }
}

fn key(p: &PaneState) -> PaneRef {
    PaneRef::try_from(p.pane_ref.as_ref().expect("pane_ref")).expect("valid ref")
}

/// Small deterministic generator (xorshift64*), so property tests need no dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}
