// SPDX-License-Identifier: MIT

use super::args::Policy;
use super::transport::Transport;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// What the observer remembers about a pane between rounds.
struct Cached {
    screen: String,
    /// The epoch second in which the screen was captured.
    captured_sec: u64,
}

/// One round's result.
pub struct Round {
    /// Every listed pane with its current screen (fresh or cached).
    pub screens: Vec<(String, String)>,
    /// How many captures were actually performed.
    pub captured: usize,
}

/// Lists panes, decides by `policy` which to capture, captures them, and reuses the previous
/// screen for the rest.
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

    pub fn round(&mut self) -> Result<Round, String> {
        // Taken before listing: output after this instant is seen as activity next round.
        let started_sec = epoch_secs();
        let dirty = self.transport.take_dirty();
        let rows = self.transport.list()?;
        let to_capture: Vec<String> = rows
            .iter()
            .filter(|row| match (self.policy, self.cache.get(&row.id)) {
                (_, None) | (Policy::All, _) => true,
                // Not older than the second of the last capture: it may have changed since.
                (Policy::SkipUnchanged, Some(c)) => row.activity >= c.captured_sec,
                (Policy::Events, Some(_)) => dirty.as_ref().is_some_and(|d| d.contains(&row.id)),
            })
            .map(|row| row.id.clone())
            .collect();
        let screens = self.transport.capture(&to_capture)?;
        for (id, screen) in to_capture.iter().zip(screens) {
            self.cache.insert(
                id.clone(),
                Cached {
                    screen,
                    captured_sec: started_sec,
                },
            );
        }
        let listed: std::collections::HashSet<&String> = rows.iter().map(|r| &r.id).collect();
        self.cache.retain(|id, _| listed.contains(id));
        Ok(Round {
            screens: rows
                .iter()
                .filter_map(|r| Some((r.id.clone(), self.cache.get(&r.id)?.screen.clone())))
                .collect(),
            captured: to_capture.len(),
        })
    }
}
