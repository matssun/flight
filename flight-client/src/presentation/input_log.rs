// SPDX-License-Identifier: MIT

use flight_state::SurfaceId;
use std::collections::VecDeque;

/// What the user typed, in order, each run tagged with the surface that had the keyboard when it
/// was typed. Bounded in bytes: when the bound is reached the keyboard is not read, so the
/// terminal's own buffer holds what comes next and nothing is dropped.
#[derive(Debug)]
pub(super) struct InputLog {
    runs: VecDeque<(SurfaceId, Vec<u8>)>,
    bytes: usize,
    limit: usize,
}

impl InputLog {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            runs: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    pub(super) fn has_room(&self) -> bool {
        self.bytes < self.limit
    }

    pub(super) fn queued_bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn push(&mut self, surface: &SurfaceId, data: Vec<u8>) {
        self.bytes = self.bytes.saturating_add(data.len());
        match self.runs.back_mut() {
            Some((last, bytes)) if last == surface => bytes.extend(data),
            _ => self.runs.push_back((surface.clone(), data)),
        }
    }

    /// The surface the oldest unsent bytes are for.
    pub(super) fn front(&self) -> Option<&SurfaceId> {
        self.runs.front().map(|(s, _)| s)
    }

    /// Up to `max` of the oldest bytes, with their surface.
    pub(super) fn pop(&mut self, max: usize) -> Option<(SurfaceId, Vec<u8>)> {
        let (surface, mut data) = self.runs.pop_front()?;
        if data.len() > max {
            let rest = data.split_off(max);
            self.runs.push_front((surface.clone(), rest));
        }
        self.bytes = self.bytes.saturating_sub(data.len());
        Some((surface, data))
    }

    /// Drop everything typed for `surface`; how many bytes that was.
    pub(super) fn discard(&mut self, surface: &SurfaceId) -> usize {
        let mut dropped = 0usize;
        self.runs.retain(|(s, data)| {
            let keep = s != surface;
            if !keep {
                dropped = dropped.saturating_add(data.len());
            }
            keep
        });
        self.bytes = self.bytes.saturating_sub(dropped);
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> SurfaceId {
        SurfaceId::new(s)
    }

    #[test]
    fn runs_keep_their_surface_and_order_and_split_at_the_maximum() {
        let mut log = InputLog::new(10);
        log.push(&id("a"), b"12".to_vec());
        log.push(&id("a"), b"34".to_vec());
        log.push(&id("b"), b"56".to_vec());
        log.push(&id("a"), b"78".to_vec());
        assert_eq!(log.queued_bytes(), 8);
        assert_eq!(log.pop(3), Some((id("a"), b"123".to_vec())));
        assert_eq!(log.front(), Some(&id("a")));
        assert_eq!(log.pop(9), Some((id("a"), b"4".to_vec())));
        assert_eq!(log.pop(9), Some((id("b"), b"56".to_vec())));
        assert_eq!(log.pop(9), Some((id("a"), b"78".to_vec())));
        assert_eq!(log.pop(9), None);
    }

    #[test]
    fn room_ends_at_the_limit_and_discarding_counts_what_was_lost() {
        let mut log = InputLog::new(4);
        log.push(&id("a"), b"12".to_vec());
        assert!(log.has_room());
        log.push(&id("b"), b"34".to_vec());
        assert!(!log.has_room());
        assert_eq!(log.discard(&id("a")), 2);
        assert!(log.has_room());
        assert_eq!(log.queued_bytes(), 2);
    }
}
