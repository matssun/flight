// SPDX-License-Identifier: MIT

use std::time::{Duration, Instant};

/// Decides what a node tells its operator about repeated connection failures: a new reason is
/// reported at once, the same reason again only as an occasional "still down" reminder.
#[derive(Debug)]
pub(crate) struct FailureLog {
    last: String,
    attempts: u64,
    since: Instant,
    last_report: Instant,
    remind_every: Duration,
}

impl FailureLog {
    pub(crate) fn new(remind_every: Duration, now: Instant) -> Self {
        Self {
            last: String::new(),
            attempts: 0,
            since: now,
            last_report: now,
            remind_every,
        }
    }

    /// A connection ended cleanly: the next failure is news again.
    pub(crate) fn reset(&mut self) {
        self.last.clear();
        self.attempts = 0;
    }

    /// Record a failed attempt that lasted `lasted`; the line to show, if any.
    pub(crate) fn failed(&mut self, why: String, lasted: Duration, now: Instant) -> Option<String> {
        self.attempts = self.attempts.saturating_add(1);
        if why != self.last {
            let line = format!("link down after {lasted:.0?}: {why}; retrying");
            self.last = why;
            self.attempts = 1;
            self.since = now;
            self.last_report = now;
            return Some(line);
        }
        if now.saturating_duration_since(self.last_report) >= self.remind_every {
            self.last_report = now;
            return Some(format!(
                "link still down: {} attempts over {:.0?}: {}",
                self.attempts,
                now.saturating_duration_since(self.since),
                self.last
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_reason_is_reported_and_the_same_reason_only_as_a_reminder() {
        let t0 = Instant::now();
        let mut log = FailureLog::new(Duration::from_secs(30), t0);
        let first = log.failed("no route".into(), Duration::ZERO, t0);
        assert!(first.is_some_and(|l| l.contains("link down") && l.contains("no route")));
        assert_eq!(
            log.failed(
                "no route".into(),
                Duration::ZERO,
                t0 + Duration::from_secs(10)
            ),
            None
        );
        let reminder = log.failed(
            "no route".into(),
            Duration::ZERO,
            t0 + Duration::from_secs(31),
        );
        assert!(
            reminder.is_some_and(|l| l.contains("still down") && l.contains("3 attempts")),
            "attempts are counted"
        );
        // A different reason is news again.
        assert!(log
            .failed(
                "refused".into(),
                Duration::ZERO,
                t0 + Duration::from_secs(32)
            )
            .is_some_and(|l| l.contains("refused")));
    }

    #[test]
    fn a_clean_end_makes_the_next_failure_news_again() {
        let t0 = Instant::now();
        let mut log = FailureLog::new(Duration::from_secs(30), t0);
        assert!(log.failed("x".into(), Duration::ZERO, t0).is_some());
        log.reset();
        assert!(log.failed("x".into(), Duration::ZERO, t0).is_some());
    }
}
