// SPDX-License-Identifier: MIT

use super::watch::{Connector, Watch};
use super::{ControlLink, PaneObserver};
use crate::{Round, TmuxServers};
use flight_state::ServerId;
use flight_tmux::TmuxError;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Control-mode observation with unchanged panes skipped: one persistent connection per
/// server instead of a process per command. A server with no connection (not connected yet,
/// broken, or no connector registered) is observed sequentially, so every round always carries
/// a correct answer for every server; the connection is re-established in the background of
/// the rounds and the first round after that is a full refresh.
pub struct ControlSkipObserver {
    servers: Arc<TmuxServers>,
    watches: BTreeMap<ServerId, Watch>,
}

impl ControlSkipObserver {
    pub fn new(servers: Arc<TmuxServers>) -> Self {
        Self {
            servers,
            watches: BTreeMap::new(),
        }
    }

    /// Observe `server` over the connection `connect` makes (and re-makes after a failure).
    pub fn watch(
        &mut self,
        server: ServerId,
        connect: impl FnMut() -> Result<Box<dyn ControlLink>, TmuxError> + Send + 'static,
    ) {
        let connector: Connector = Box::new(connect);
        self.watches.insert(server, Watch::new(connector));
    }
}

impl PaneObserver for ControlSkipObserver {
    fn observe(&mut self, now: u64) -> Vec<Round> {
        let mut rounds = Vec::new();
        for server in self.servers.server_ids() {
            let controlled = self
                .watches
                .get_mut(&server)
                .and_then(|watch| watch.observe(now));
            let round = match controlled {
                Some(outcome) => Some(Round {
                    server: server.clone(),
                    now,
                    outcome,
                }),
                None => self.servers.observe_one(&server, now),
            };
            rounds.extend(round);
        }
        rounds
    }

    fn take_notes(&mut self) -> Vec<String> {
        self.watches
            .iter_mut()
            .flat_map(|(server, watch)| {
                watch
                    .take_notes()
                    .into_iter()
                    .map(move |note| format!("{server}: {note}"))
            })
            .collect()
    }
}
