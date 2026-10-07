// SPDX-License-Identifier: MIT

use super::args::Policy;
use super::transport::Transport;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// What the observer remembers about a pane between rounds.
struct Cached {
    screen: String,
    /// The process behind the pane: a reused `%id` with another pid is a different pane.
    pid: u64,
    /// The epoch second in which the screen was captured.
    captured_sec: u64,
}

/// One round's result.
pub struct Round {
    /// Every listed pane whose screen is known (fresh or cached).
    pub screens: Vec<(String, String)>,
    /// How many captures were actually performed.
    pub captured: usize,
}

/// Lists panes, decides by `policy` which to capture, captures them, and reuses the previous
/// screen for the rest.
///
/// Skipping is conservative: a pane is reused only when it is known, has the same process,
/// and its window's activity is provably older than its last capture. Anything uncertain is
/// captured. Any transport error discards the whole cache, so what follows an error (after
/// `recover`) is a full refresh, never an optimisation built on pre-error beliefs.
pub struct Observer<T: Transport> {
    transport: T,
    policy: Policy,
    cache: HashMap<String, Cached>,
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl<T: Transport> Observer<T> {
    pub fn new(transport: T, policy: Policy) -> Self {
        Self {
            transport,
            policy,
            cache: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Discard everything believed about the panes and re-establish the transport.
    pub fn recover(&mut self) -> Result<(), String> {
        self.cache.clear();
        self.transport.recover()
    }

    pub fn round(&mut self) -> Result<Round, String> {
        let result = self.round_inner();
        if result.is_err() {
            self.cache.clear();
        }
        result
    }

    fn round_inner(&mut self) -> Result<Round, String> {
        // Taken before listing: output after this instant is seen as activity next round.
        let started_sec = epoch_secs();
        let rows = self.transport.list()?;
        let to_capture: Vec<&_> = rows
            .iter()
            .filter(|row| match (self.policy, self.cache.get(&row.id)) {
                (_, None) | (Policy::All, _) => true,
                (Policy::SkipUnchanged, Some(c)) => {
                    // No activity timestamp, or not older than the second of the last
                    // capture: it may have changed since.
                    c.pid != row.pid || row.activity == 0 || row.activity >= c.captured_sec
                }
            })
            .collect();
        let ids: Vec<String> = to_capture.iter().map(|r| r.id.clone()).collect();
        let screens = self.transport.capture(&ids)?;
        if screens.len() != ids.len() {
            return Err(format!(
                "asked for {} captures, got {}",
                ids.len(),
                screens.len()
            ));
        }
        for (row, screen) in to_capture.iter().zip(screens) {
            match screen {
                Some(screen) => {
                    self.cache.insert(
                        row.id.clone(),
                        Cached {
                            screen,
                            pid: row.pid,
                            captured_sec: started_sec,
                        },
                    );
                }
                // Unknown now: forget it so the next round captures it again.
                None => {
                    self.cache.remove(&row.id);
                }
            }
        }
        let listed: HashSet<&String> = rows.iter().map(|r| &r.id).collect();
        self.cache.retain(|id, _| listed.contains(id));
        Ok(Round {
            screens: rows
                .iter()
                .filter_map(|r| Some((r.id.clone(), self.cache.get(&r.id)?.screen.clone())))
                .collect(),
            captured: ids.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::PaneRow;
    use super::*;

    /// A scripted tmux: panes with a screen, a pid and an activity second.
    #[derive(Default)]
    struct Fake {
        rows: Vec<PaneRow>,
        screens: HashMap<String, String>,
        fail_list: bool,
        fail_capture: HashSet<String>,
        captured: Vec<String>,
        recovered: u32,
    }

    impl Fake {
        fn pane(&mut self, id: &str, pid: u64, activity: u64, screen: &str) {
            self.rows.retain(|r| r.id != id);
            self.rows.push(PaneRow {
                id: id.into(),
                pid,
                activity,
            });
            self.screens.insert(id.into(), screen.into());
        }
    }

    impl Transport for Fake {
        fn list(&mut self) -> Result<Vec<PaneRow>, String> {
            if self.fail_list {
                return Err("no server".into());
            }
            Ok(self.rows.clone())
        }
        fn capture(&mut self, ids: &[String]) -> Result<Vec<Option<String>>, String> {
            self.captured.extend(ids.iter().cloned());
            Ok(ids
                .iter()
                .map(|id| {
                    (!self.fail_capture.contains(id))
                        .then(|| self.screens.get(id).cloned())
                        .flatten()
                })
                .collect())
        }
        fn recover(&mut self) -> Result<(), String> {
            self.recovered += 1;
            Ok(())
        }
    }

    const OLD: u64 = 1; // activity far older than any capture
    const NEW: u64 = u64::MAX; // activity "after" any capture

    fn observer(fake: Fake) -> Observer<Fake> {
        Observer::new(fake, Policy::SkipUnchanged)
    }

    fn screen_of(round: &Round, id: &str) -> Option<String> {
        round
            .screens
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, s)| s.clone())
    }

    #[test]
    fn unchanged_panes_are_skipped_changed_ones_captured() {
        let mut f = Fake::default();
        f.pane("%1", 10, OLD, "one");
        f.pane("%2", 11, OLD, "two");
        let mut o = observer(f);
        assert_eq!(o.round().unwrap().captured, 2);
        o.transport().captured.clear();
        o.transport().pane("%2", 11, NEW, "two!");
        let r = o.round().unwrap();
        assert_eq!(r.captured, 1);
        assert_eq!(o.transport().captured, vec!["%2".to_owned()]);
        assert_eq!(screen_of(&r, "%1").as_deref(), Some("one"));
        assert_eq!(screen_of(&r, "%2").as_deref(), Some("two!"));
    }

    #[test]
    fn a_reused_pane_id_with_a_new_process_is_captured_even_if_activity_looks_old() {
        let mut f = Fake::default();
        f.pane("%3", 10, OLD, "before");
        let mut o = observer(f);
        o.round().unwrap();
        o.transport().pane("%3", 99, OLD, "after");
        let r = o.round().unwrap();
        assert_eq!(r.captured, 1);
        assert_eq!(screen_of(&r, "%3").as_deref(), Some("after"));
    }

    #[test]
    fn missing_activity_is_uncertain_and_captured() {
        let mut f = Fake::default();
        f.pane("%1", 10, 0, "x");
        let mut o = observer(f);
        o.round().unwrap();
        assert_eq!(o.round().unwrap().captured, 1);
    }

    #[test]
    fn a_failed_capture_is_unknown_and_retried_next_round() {
        let mut f = Fake::default();
        f.pane("%1", 10, OLD, "x");
        f.fail_capture.insert("%1".into());
        let mut o = observer(f);
        let r = o.round().unwrap();
        assert!(screen_of(&r, "%1").is_none());
        o.transport().fail_capture.clear();
        let r = o.round().unwrap();
        assert_eq!(r.captured, 1);
        assert_eq!(screen_of(&r, "%1").as_deref(), Some("x"));
    }

    #[test]
    fn removed_panes_leave_and_return_fresh() {
        let mut f = Fake::default();
        f.pane("%1", 10, OLD, "x");
        let mut o = observer(f);
        o.round().unwrap();
        o.transport().rows.clear();
        assert!(o.round().unwrap().screens.is_empty());
        o.transport().pane("%1", 10, OLD, "x again");
        assert_eq!(o.round().unwrap().captured, 1);
    }

    #[test]
    fn an_error_discards_the_cache_so_the_next_round_is_a_full_refresh() {
        let mut f = Fake::default();
        f.pane("%1", 10, OLD, "x");
        f.pane("%2", 11, OLD, "y");
        let mut o = observer(f);
        o.round().unwrap();
        o.transport().fail_list = true;
        assert!(o.round().is_err());
        o.transport().fail_list = false;
        o.recover().unwrap();
        assert_eq!(o.transport().recovered, 1);
        assert_eq!(o.round().unwrap().captured, 2);
    }
}
