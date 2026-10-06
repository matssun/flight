// SPDX-License-Identifier: MIT

//! The central property: an initial snapshot plus every emitted delta reproduces exactly
//! the state a full snapshot reports, over many generated transition sequences.

mod support;

use flight_node::{NodeCore, PaneObservation, Round, ServerOutcome, Unavailable};
use flight_proto::Step;
use flight_state::{HostId, ServerId};
use support::*;

const SCREENS: [&str; 4] = [PERMIT_SCREEN, IDLE_SCREEN, BUSY_SCREEN, ""];

fn random_round(rng: &mut Rng, servers: &[ServerId], now: u64) -> Round {
    let server_id = servers[rng.below(servers.len() as u64) as usize].clone();
    if rng.chance(12) {
        let why = match rng.below(3) {
            0 => Unavailable::NoServer,
            1 => Unavailable::TmuxMissing,
            _ => Unavailable::Failed("flaky".into()),
        };
        return down(&server_id, now, why);
    }
    // A small pool of pane ids, so ids vanish and come back, sometimes with another pid.
    let panes: Vec<PaneObservation> = (1..=4)
        .filter_map(|n| {
            if !rng.chance(65) {
                return None;
            }
            let screen = SCREENS[rng.below(SCREENS.len() as u64) as usize];
            let pid = 100 + rng.below(3) as u32;
            Some(obs(&format!("%{n}"), pid, screen, rng.chance(20)))
        })
        .collect();
    Round {
        server: server_id,
        now,
        outcome: ServerOutcome::Observed(panes),
    }
}

#[test]
fn snapshot_plus_deltas_equals_the_final_snapshot() {
    let servers = [ServerId::new("flight"), ServerId::new("work")];
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut core = NodeCore::new(HostId::new("node-1"), inc((seed % 250) as u8 + 1));
        let mut mirror = Mirror::default();
        mirror.load(&core.snapshot());
        let mut now = 1_000;
        for step in 0..60 {
            now += 1 + rng.below(20);
            for delta in core.apply(random_round(&mut rng, &servers, now)) {
                assert_eq!(mirror.apply(&delta), Step::Apply, "seed {seed} step {step}");
            }
            assert!(
                mirror.matches(&core.state()),
                "seed {seed} step {step}: diverged"
            );
            // An occasional resync re-baselines both sides; deltas keep applying afterwards.
            if rng.chance(8) {
                mirror.load(&core.snapshot());
            }
        }
    }
}

#[test]
fn a_receiver_that_joins_late_converges_with_one_snapshot() {
    let servers = [ServerId::new("flight")];
    let mut rng = Rng(42);
    let mut core = core();
    let mut now = 1_000;
    for _ in 0..40 {
        now += 5;
        core.apply(random_round(&mut rng, &servers, now));
    }
    let mut late = Mirror::default();
    late.load(&core.snapshot());
    for _ in 0..40 {
        now += 5;
        for delta in core.apply(random_round(&mut rng, &servers, now)) {
            assert_eq!(late.apply(&delta), Step::Apply);
        }
    }
    assert!(late.matches(&core.state()));
}

#[test]
fn losing_one_delta_is_detected_and_one_snapshot_repairs_it() {
    let s = server();
    let mut core = core();
    let mut mirror = Mirror::default();
    mirror.load(&core.snapshot());
    let first = core.apply(round(&s, 100, vec![obs("%1", 1, PERMIT_SCREEN, false)]));
    let second = core.apply(round(&s, 110, vec![obs("%1", 1, BUSY_SCREEN, false)]));
    // The first delta never arrives.
    assert_eq!(mirror.apply(&second[0]), Step::Resync);
    assert!(!first.is_empty());
    mirror.load(&core.snapshot());
    assert!(mirror.matches(&core.snapshot()));
}
