// SPDX-License-Identifier: MIT

/// What a receiver does with the next delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// In order for the current generation: apply it.
    Apply,
    /// Anything else: stop applying, ask for a fresh snapshot.
    Resync,
}

/// Receiver-side bookkeeping for `Snapshot(generation = N)`, then `Delta(N, 1)`, `Delta(N, 2)`…
///
/// There is deliberately no repair: a missing, duplicate or foreign-generation delta, or a
/// reconnect, means "ask for a snapshot". After a `Resync` every delta also answers `Resync`
/// until the next snapshot arrives, so a stale tail can never be half-applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicationCursor {
    /// `Some((generation, next expected sequence))` once a snapshot is held and the stream is sound.
    position: Option<(u64, u64)>,
}

impl ReplicationCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot is always accepted: it is authoritative.
    pub fn on_snapshot(&mut self, generation: u64) {
        self.position = Some((generation, 1));
    }

    pub fn on_delta(&mut self, generation: u64, sequence: u64) -> Step {
        match self.position {
            Some((g, next)) if g == generation && next == sequence => {
                self.position = Some((g, next.saturating_add(1)));
                Step::Apply
            }
            _ => {
                self.position = None;
                Step::Resync
            }
        }
    }

    /// The connection dropped: nothing held can be trusted.
    pub fn reset(&mut self) {
        self.position = None;
    }

    pub fn in_sync(&self) -> bool {
        self.position.is_some()
    }
}
