// SPDX-License-Identifier: MIT

use std::time::{Duration, Instant};

/// Tracks an unbroken run of local "unreachable" failures. Pure, so it is tested with
/// synthetic time.
#[derive(Debug, Clone)]
pub(crate) struct UnreachableWatch {
    limit: Duration,
    since: Option<Instant>,
}

impl UnreachableWatch {
    pub(crate) fn new(limit: Duration) -> Self {
        Self { limit, since: None }
    }

    /// How long the current unbroken run of failures has lasted at `now`.
    pub(crate) fn window(&self, now: Instant) -> Duration {
        self.since.map_or(Duration::ZERO, |s| now.duration_since(s))
    }

    /// Record the outcome of an attempt at `now`; true when the window has been exceeded.
    pub(crate) fn record(&mut self, unreachable: bool, now: Instant) -> bool {
        if !unreachable {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.duration_since(since) >= self.limit
    }
}

/// `limit` plus up to a quarter, so a fleet of nodes does not restart in lock step.
pub(crate) fn jittered(limit: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    // Saturating throughout: this runs in production and must never panic.
    let permille = u32::try_from(nanos % 1000).unwrap_or(0);
    limit.saturating_add(limit.saturating_mul(permille) / 4000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_of_unreachable_failures_trips_the_watch_only_after_the_limit() {
        let t0 = Instant::now();
        let mut w = UnreachableWatch::new(Duration::from_secs(60));
        assert!(!w.record(true, t0));
        assert!(!w.record(true, t0 + Duration::from_secs(59)));
        assert!(w.record(true, t0 + Duration::from_secs(60)));
        assert_eq!(
            w.window(t0 + Duration::from_secs(61)),
            Duration::from_secs(61)
        );
    }

    #[test]
    fn any_other_outcome_restarts_the_window() {
        let t0 = Instant::now();
        let mut w = UnreachableWatch::new(Duration::from_secs(60));
        assert!(!w.record(true, t0));
        // The orchestrator being down, refusing or slow is not the condition: reset.
        assert!(!w.record(false, t0 + Duration::from_secs(50)));
        assert!(!w.record(true, t0 + Duration::from_secs(70)));
        assert!(!w.record(true, t0 + Duration::from_secs(120)));
        assert!(w.record(true, t0 + Duration::from_secs(130)));
    }

    #[test]
    fn jitter_adds_at_most_a_quarter() {
        for _ in 0..50 {
            let j = jittered(Duration::from_secs(60));
            assert!(
                j >= Duration::from_secs(60) && j <= Duration::from_secs(75),
                "{j:?}"
            );
        }
    }
}
