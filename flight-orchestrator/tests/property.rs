// SPDX-License-Identifier: MIT

//! Generated histories of node activity, drops, crashes, silence and orchestrator restarts.
//! Properties:
//! - every in-sync UI mirror equals the orchestrator's fleet image, at every step;
//! - once nodes have answered a resync, the orchestrator holds exactly what each connected
//!   node publishes;
//! - an orchestrator that restarts and is rebuilt from node snapshots alone reconstructs
//!   exactly the image of the nodes that are live (it stores nothing durable).

mod support;

use flight_node::Unavailable;
use flight_orchestrator::UiId;
use flight_proto::{NodeStatusCode, NodeView};
use support::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

const SCREENS: [&str; 3] = [PERMIT_SCREEN, IDLE_SCREEN, BUSY_SCREEN];

fn random_round(rng: &mut Rng, now: u64) -> flight_node::Round {
    if rng.chance(10) {
        let why = match rng.below(3) {
            0 => Unavailable::NoServer,
            1 => Unavailable::TmuxMissing,
            _ => Unavailable::Failed("flaky".into()),
        };
        return down(now, why);
    }
    let panes = (1..=3)
        .filter_map(|n| {
            if !rng.chance(70) {
                return None;
            }
            let screen = SCREENS[rng.below(3) as usize];
            Some(obs(&format!("%{n}"), 10 + rng.below(2) as u32, screen))
        })
        .collect();
    round(now, panes)
}

fn online(views: &[NodeView]) -> Vec<NodeView> {
    views
        .iter()
        .filter(|v| v.status == NodeStatusCode::Online as i32)
        .cloned()
        .collect()
}

fn step(w: &mut World, rng: &mut Rng) {
    let i = rng.below(w.nodes.len() as u64) as usize;
    match rng.below(100) {
        0..=44 => {
            let r = random_round(rng, w.now);
            if rng.chance(15) {
                w.drop_node_frames = true;
                w.observe(i, r);
                w.drop_node_frames = false;
            } else {
                w.observe(i, r);
            }
        }
        45..=59 => {
            if w.nodes[i].conn.is_none() || rng.chance(10) {
                w.connect(i);
            }
        }
        60..=69 => w.disconnect(i),
        70..=74 => {
            // The node process crashes and comes back with a new incarnation.
            w.disconnect(i);
            w.nodes[i].restart("renamed");
        }
        75..=84 => {
            if rng.chance(30) {
                w.mute(i);
            } else {
                w.unmute(i);
            }
            let secs = 1 + rng.below(12);
            w.advance(secs);
        }
        85..=92 => w.subscribe(UiId(1 + rng.below(2))),
        _ => {}
    }
}

#[test]
fn ui_mirrors_always_equal_the_orchestrators_image_and_converge_with_nodes() {
    for seed in 1..=150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut w = World::new(vec![
            SimNode::new("node-a", "mini-1", 1),
            SimNode::new("node-b", "mini-2", 1),
            SimNode::new("node-c", "mini-2", 1),
        ]);
        w.subscribe(UiId(1));
        for n in 0..3 {
            w.connect(n);
        }
        for s in 0..60 {
            step(&mut w, &mut rng);
            for (ui, mirror) in &w.uis {
                if mirror.in_sync() {
                    assert_eq!(
                        mirror.views(),
                        w.views(),
                        "seed {seed} step {s}: ui {ui:?} diverged"
                    );
                }
            }
            if s % 7 == 0 {
                // Unmuted, connected nodes answer a resync; then the image equals the node.
                w.muted.clear();
                w.flush();
                for node in &w.nodes {
                    let Some(view) = w.node_view(node.id.as_str()) else {
                        continue;
                    };
                    if node.conn.is_some() && view.status != NodeStatusCode::Disconnected as i32 {
                        let truth = node.session.core().state();
                        assert_eq!(
                            view.panes, truth.panes,
                            "seed {seed} step {s}: {} panes",
                            node.id
                        );
                        assert_eq!(
                            view.servers, truth.servers,
                            "seed {seed} step {s}: {} servers",
                            node.id
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn an_orchestrator_rebuilt_from_node_snapshots_reconstructs_the_live_image() {
    for seed in 1..=100u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let mut w = World::new(vec![
            SimNode::new("node-a", "mini-1", 1),
            SimNode::new("node-b", "mini-2", 1),
            SimNode::new("node-c", "mini-3", 1),
        ]);
        for n in 0..3 {
            w.connect(n);
        }
        for _ in 0..40 {
            step(&mut w, &mut rng);
        }
        w.muted.clear();
        w.flush();
        let live: Vec<usize> = (0..3).filter(|i| w.nodes[*i].conn.is_some()).collect();
        let before = online(&w.views());
        w.restart_orchestrator();
        for i in &live {
            w.connect(*i);
        }
        assert_eq!(
            online(&w.views()),
            before,
            "seed {seed}: rebuilt image differs"
        );
    }
}
