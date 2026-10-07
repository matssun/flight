// SPDX-License-Identifier: MIT

//! The bounded outbox: backlog never grows past its capacity; falling behind means resync.

use flight_transport::outbox::{Outbox, PushError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn no_resync() {}

#[tokio::test]
async fn items_come_out_in_order() {
    let o = Outbox::new(8);
    o.push_delta(1).expect("push");
    o.push_reliable(2).expect("push");
    o.push_delta(3).expect("push");
    assert_eq!(o.next(&no_resync).await, Some(1));
    assert_eq!(o.next(&no_resync).await, Some(2));
    assert_eq!(o.next(&no_resync).await, Some(3));
}

#[test]
fn the_backlog_never_exceeds_capacity() {
    let o = Outbox::new(16);
    for i in 0..10_000 {
        o.push_delta(i).expect("deltas never fail");
        assert!(o.len() <= 16);
    }
    assert!(o.is_overflowed());
}

#[test]
fn overflow_discards_queued_deltas_and_keeps_reliable_items() {
    let o = Outbox::new(4);
    o.push_reliable("response").expect("reliable");
    for i in 0..10 {
        o.push_delta(if i == 0 { "d0" } else { "later" })
            .expect("delta");
    }
    assert!(o.is_overflowed());
    assert_eq!(o.len(), 1, "only the reliable item survives");
}

#[tokio::test]
async fn a_snapshot_is_requested_when_deltas_were_discarded() {
    let o = Arc::new(Outbox::new(2));
    for i in 0..5 {
        o.push_delta(format!("delta {i}")).expect("delta");
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let (o2, c2) = (o.clone(), calls.clone());
    let resync = move || {
        c2.fetch_add(1, Ordering::SeqCst);
        // The producer queues the snapshot and marks the stream in sync, together.
        o2.push_reliable("snapshot".to_owned()).expect("snapshot");
        o2.clear_overflow();
    };
    assert_eq!(o.next(&resync).await.as_deref(), Some("snapshot"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // After the snapshot, deltas flow again.
    o.push_delta("delta after".to_owned()).expect("delta");
    assert_eq!(o.next(&resync).await.as_deref(), Some("delta after"));
}

#[test]
fn deltas_are_dropped_while_a_resync_is_pending() {
    let o = Outbox::new(2);
    for i in 0..3 {
        o.push_delta(i).expect("delta");
    }
    assert!(o.is_overflowed());
    o.push_delta(99).expect("dropped, not an error");
    assert_eq!(o.len(), 0);
}

#[test]
fn reliable_items_are_bounded_and_refused_when_the_peer_is_not_reading() {
    let o = Outbox::new(3);
    for i in 0..3 {
        o.push_reliable(i).expect("room");
    }
    assert_eq!(o.push_reliable(4), Err(PushError::Full));
    assert_eq!(o.len(), 3);
}

#[test]
fn a_full_queue_of_deltas_makes_room_for_a_reliable_item() {
    let o = Outbox::new(3);
    for i in 0..3 {
        o.push_delta(i).expect("delta");
    }
    o.push_reliable(100).expect("evicts deltas");
    assert!(o.is_overflowed());
    assert_eq!(o.len(), 1);
}

#[test]
fn coalesced_items_never_accumulate() {
    let o = Outbox::new(8);
    for i in 0..1000 {
        o.push_coalesced(0, i).expect("heartbeat");
    }
    o.push_coalesced(1, -1).expect("resync request");
    o.push_coalesced(1, -2).expect("resync request");
    assert_eq!(o.len(), 2, "one heartbeat and one resync request");
}

#[tokio::test]
async fn a_coalesced_item_keeps_the_newest_value() {
    let o = Outbox::new(8);
    o.push_coalesced(0, "old").expect("hb");
    o.push_coalesced(0, "new").expect("hb");
    assert_eq!(o.next(&no_resync).await, Some("new"));
}

#[tokio::test]
async fn closing_drains_what_is_queued_then_ends() {
    let o = Outbox::new(8);
    o.push_reliable("goodbye").expect("push");
    o.close();
    assert_eq!(o.push_reliable("late"), Err(PushError::Closed));
    assert_eq!(o.push_delta("late"), Err(PushError::Closed));
    assert_eq!(o.next(&no_resync).await, Some("goodbye"));
    assert_eq!(o.next(&no_resync).await, None);
}

#[tokio::test]
async fn a_waiting_consumer_wakes_when_something_arrives() {
    let o = Arc::new(Outbox::new(8));
    let o2 = o.clone();
    let waiter = tokio::spawn(async move { o2.next(&no_resync).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    o.push_reliable(7).expect("push");
    assert_eq!(waiter.await.expect("join"), Some(7));
}
