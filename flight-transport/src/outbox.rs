// SPDX-License-Identifier: MIT

//! A bounded outbound queue with replication-aware overflow.
//!
//! Invariant: the replication backlog is bounded. Falling behind causes resynchronization,
//! never unbounded buffering: a delta is disposable (a snapshot is authoritative), so when a
//! peer cannot keep up the queued deltas are discarded and the producer is asked for a fresh
//! snapshot instead.
//!
//! Three traffic classes:
//! - [`Outbox::push_delta`]: disposable replication. On overflow every queued delta is
//!   dropped and the outbox is marked *overflowed*; further deltas are dropped until the
//!   consumer's `resync` callback has put a snapshot in and cleared the mark.
//! - [`Outbox::push_reliable`]: responses, snapshots, hellos. Bounded: if the queue is full
//!   of reliable items the push fails and the caller decides (typically: close the peer).
//! - [`Outbox::push_coalesced`]: heartbeats and resync requests; at most one of each kind
//!   is ever queued.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use tokio::sync::Notify;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Delta,
    Reliable,
    Coalesced(u8),
}

struct State<T> {
    queue: VecDeque<(Class, T)>,
    closed: bool,
    overflowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError {
    /// The outbox is closed.
    Closed,
    /// Full of reliable items: the peer is not reading.
    Full,
}

pub struct Outbox<T> {
    state: Mutex<State<T>>,
    notify: Notify,
    capacity: usize,
}

impl<T> Outbox<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                closed: false,
                overflowed: false,
            }),
            notify: Notify::new(),
            capacity: capacity.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn evict_deltas(state: &mut State<T>) {
        state.queue.retain(|(class, _)| *class != Class::Delta);
        state.overflowed = true;
    }

    /// Disposable replication. Never fails for lack of room: it degrades to "resync needed".
    pub fn push_delta(&self, item: T) -> Result<(), PushError> {
        let mut state = self.lock();
        if state.closed {
            return Err(PushError::Closed);
        }
        if state.overflowed {
            return Ok(());
        }
        if state.queue.len() >= self.capacity {
            Self::evict_deltas(&mut state);
            drop(state);
            self.notify.notify_one();
            return Ok(());
        }
        state.queue.push_back((Class::Delta, item));
        drop(state);
        self.notify.notify_one();
        Ok(())
    }

    /// Items that must be delivered. Deltas make room first (forcing a resync); if the queue
    /// is still full of reliable items the push fails.
    pub fn push_reliable(&self, item: T) -> Result<(), PushError> {
        let mut state = self.lock();
        if state.closed {
            return Err(PushError::Closed);
        }
        if state.queue.len() >= self.capacity {
            Self::evict_deltas(&mut state);
            if state.queue.len() >= self.capacity {
                return Err(PushError::Full);
            }
        }
        state.queue.push_back((Class::Reliable, item));
        drop(state);
        self.notify.notify_one();
        Ok(())
    }

    /// At most one queued item per `kind`: a newer one replaces an older one still waiting.
    /// Dropped silently when there is no room (these are always repeatable).
    pub fn push_coalesced(&self, kind: u8, item: T) -> Result<(), PushError> {
        let mut state = self.lock();
        if state.closed {
            return Err(PushError::Closed);
        }
        let class = Class::Coalesced(kind);
        if let Some(slot) = state.queue.iter_mut().find(|(c, _)| *c == class) {
            slot.1 = item;
        } else if state.queue.len() < self.capacity {
            state.queue.push_back((class, item));
        }
        drop(state);
        self.notify.notify_one();
        Ok(())
    }

    /// Mark the stream in sync again. The caller must do this together with queueing the
    /// snapshot, under whatever lock orders snapshots against new deltas.
    pub fn clear_overflow(&self) {
        self.lock().overflowed = false;
        self.notify.notify_one();
    }

    pub fn is_overflowed(&self) -> bool {
        self.lock().overflowed
    }

    pub fn len(&self) -> usize {
        self.lock().queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&self) {
        self.lock().closed = true;
        self.notify.notify_one();
    }

    /// The next item to send. When deltas were discarded, `resync` is called first; it must
    /// queue a snapshot and call [`Outbox::clear_overflow`]. `None` once closed and drained.
    pub async fn next(&self, resync: &(dyn Fn() + Send + Sync)) -> Option<T> {
        loop {
            enum Step<T> {
                Resync,
                Item(T),
                Done,
                Wait,
            }
            let step = {
                let mut state = self.lock();
                if state.overflowed && !state.closed {
                    Step::Resync
                } else if let Some((_, item)) = state.queue.pop_front() {
                    Step::Item(item)
                } else if state.closed {
                    Step::Done
                } else {
                    Step::Wait
                }
            };
            match step {
                Step::Resync => resync(),
                Step::Item(item) => return Some(item),
                Step::Done => return None,
                Step::Wait => self.notify.notified().await,
            }
        }
    }
}
