// SPDX-License-Identifier: MIT

use flight_proto::{ReplicationCursor, Step};

fn synced(generation: u64) -> ReplicationCursor {
    let mut c = ReplicationCursor::new();
    c.on_snapshot(generation);
    c
}

#[test]
fn deltas_apply_in_order_after_a_snapshot() {
    let mut c = synced(7);
    assert_eq!(c.on_delta(7, 1), Step::Apply);
    assert_eq!(c.on_delta(7, 2), Step::Apply);
    assert!(c.in_sync());
}

#[test]
fn deltas_before_any_snapshot_ask_for_one() {
    assert_eq!(ReplicationCursor::new().on_delta(1, 1), Step::Resync);
}

#[test]
fn a_gap_a_duplicate_or_a_foreign_generation_forces_resync() {
    for (g, s) in [(7, 3), (7, 1), (8, 2)] {
        let mut c = synced(7);
        assert_eq!(c.on_delta(7, 1), Step::Apply);
        assert_eq!(
            c.on_delta(g, s),
            Step::Resync,
            "generation {g} sequence {s}"
        );
        assert!(!c.in_sync());
    }
}

#[test]
fn after_resync_the_stale_tail_is_never_half_applied() {
    let mut c = synced(7);
    assert_eq!(c.on_delta(7, 5), Step::Resync);
    assert_eq!(c.on_delta(7, 1), Step::Resync);
    assert_eq!(c.on_delta(7, 2), Step::Resync);
}

#[test]
fn a_new_snapshot_recovers_and_may_use_any_generation() {
    let mut c = synced(7);
    assert_eq!(c.on_delta(7, 9), Step::Resync);
    c.on_snapshot(2);
    assert_eq!(c.on_delta(2, 1), Step::Apply);
}

#[test]
fn reconnect_invalidates_everything() {
    let mut c = synced(7);
    c.reset();
    assert_eq!(c.on_delta(7, 1), Step::Resync);
}
