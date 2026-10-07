// SPDX-License-Identifier: MIT

use crate::shared::now;
use crate::NodeLink;
use flight_node::PaneObserver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// A round slower than the poll interval is mentioned at most this often.
const SLOW_ROUND_REMINDER: Duration = Duration::from_secs(30);

/// How long to wait after a round that took `took` before starting the next, so that rounds
/// start about `interval` apart (the picture is never staler than the interval plus one round)
/// but the node is idle at least as long as it was busy, however slow tmux is.
pub(crate) fn next_delay(interval: Duration, took: Duration) -> Duration {
    interval.saturating_sub(took).max(took)
}

/// Poll the node's tmux servers through `observer` on a blocking thread (never on the runtime,
/// never under the session lock) and feed what was seen to the link, until `stop` flips.
/// Things the observer wants the operator to know (a strategy degrading to its fallback or
/// recovering) are passed to the link's log. The next round starts
/// about `interval` after the previous one started (never closer than the previous round's duration); a round that takes longer than the interval is
/// reported to the operator, because it, not the interval, then sets how stale the picture is.
pub async fn run_observer(
    link: Arc<NodeLink>,
    observer: Box<dyn PaneObserver>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) {
    let observer = Arc::new(Mutex::new(observer));
    let mut last_slow_note: Option<Instant> = None;
    loop {
        let polled = observer.clone();
        let started = Instant::now();
        let observed = tokio::task::spawn_blocking(move || {
            // A panic in an earlier round must not end observation for good.
            let mut observer = polled.lock().unwrap_or_else(|e| e.into_inner());
            (observer.observe(now()), observer.take_notes())
        })
        .await;
        let took = started.elapsed();
        if let Ok((rounds, notes)) = observed {
            link.observe(rounds);
            for note in notes {
                link.note(note);
            }
        }
        let due = last_slow_note.is_none_or(|t| t.elapsed() >= SLOW_ROUND_REMINDER);
        if took > interval && due {
            link.note(format!(
                "observation round took {took:.0?}, longer than the {interval:.0?} poll interval: state changes are seen up to {:.0?} late",
                took.saturating_add(next_delay(interval, took))
            ));
            last_slow_note = Some(Instant::now());
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(next_delay(interval, took)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn rounds_start_one_interval_apart_when_tmux_is_fast() {
        assert_eq!(next_delay(ms(1000), ms(100)), ms(900));
        assert_eq!(next_delay(ms(1000), ms(500)), ms(500));
        assert_eq!(next_delay(ms(2000), ms(0)), ms(2000));
    }

    #[test]
    fn a_slow_round_is_followed_by_at_least_as_much_idle_time() {
        assert_eq!(next_delay(ms(250), ms(800)), ms(800));
        assert_eq!(next_delay(ms(1000), ms(2000)), ms(2000));
        // Never a busy loop, whatever the interval.
        assert!(next_delay(ms(50), ms(400)) >= ms(400));
    }
}
