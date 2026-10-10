// SPDX-License-Identifier: MIT

//! Reading the servers: what the node publishes, one round per server.

use super::{TmuxServers, NO_SERVER_MARKERS, SCRAPE_LINES};
use crate::{pane_agent, PaneObservation, Round, ServerOutcome, Unavailable};
use flight_state::ServerId;
use flight_tmux::{Tmux, TmuxError, TmuxRunner};
use std::io::ErrorKind;

impl TmuxServers {
    pub fn server_ids(&self) -> Vec<ServerId> {
        self.servers.keys().cloned().collect()
    }

    /// Observe every server once. A failing server yields an `Unavailable` round; it never
    /// stops the others.
    pub fn observe(&self, now: u64) -> Vec<Round> {
        self.servers
            .keys()
            .filter_map(|server| self.observe_one(server, now))
            .collect()
    }

    /// Observe one server with plain per-command tmux calls: the reference path, and the
    /// fallback of every other observer.
    pub fn observe_one(&self, server: &ServerId, now: u64) -> Option<Round> {
        let tmux = self.servers.get(server)?;
        Some(Round {
            server: server.clone(),
            now,
            outcome: observe_server(tmux),
        })
    }

    /// The panes the node publishes (those of Flight's sessions and those running an agent),
    /// from every server. A server with no tmux running contributes none; any other failure is
    /// an error, never an empty answer.
    pub(super) fn published(&self) -> Result<Vec<(ServerId, flight_tmux::PaneInfo)>, String> {
        let mut out = Vec::new();
        for (server, tmux) in &self.servers {
            match tmux.list_panes() {
                Ok(panes) => out.extend(
                    panes
                        .into_iter()
                        .filter(|p| pane_agent(p).is_some())
                        .map(|p| (server.clone(), p)),
                ),
                Err(e) => match unavailable(&e) {
                    Unavailable::NoServer => {}
                    other => return Err(format!("{server}: {other:?}")),
                },
            }
        }
        Ok(out)
    }
}

fn observe_server(tmux: &Tmux<Box<dyn TmuxRunner + Send + Sync>>) -> ServerOutcome {
    let infos = match tmux.list_panes() {
        Ok(infos) => infos,
        Err(e) => return ServerOutcome::Unavailable(unavailable(&e)),
    };
    let panes = infos
        .into_iter()
        .filter_map(|info| {
            let agent = pane_agent(&info)?;
            // A failed capture classifies with no screen evidence rather than dropping the pane.
            let screen_lines = tmux
                .capture_pane(&info.pane_id, true, Some(SCRAPE_LINES))
                .map(|t| t.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            let focused = info.focused;
            Some(PaneObservation::from_info(
                info,
                agent,
                screen_lines,
                focused,
            ))
        })
        .collect();
    ServerOutcome::Observed(panes)
}

fn unavailable(e: &TmuxError) -> Unavailable {
    match e {
        TmuxError::Spawn(io) if io.kind() == ErrorKind::NotFound => Unavailable::TmuxMissing,
        TmuxError::Failed { stderr, .. }
            if NO_SERVER_MARKERS
                .iter()
                .any(|m| stderr.to_lowercase().contains(m)) =>
        {
            Unavailable::NoServer
        }
        other => Unavailable::Failed(other.to_string()),
    }
}
