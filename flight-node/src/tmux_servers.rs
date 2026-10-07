// SPDX-License-Identifier: MIT

use crate::{Control, ControlError, PaneObservation, Round, ServerOutcome, Unavailable};
use flight_classify::detect_agent;
use flight_proto::ErrorKindCode;
use flight_state::{PaneId, ServerId};
use flight_tmux::{Tmux, TmuxError, TmuxRunner};
use std::collections::BTreeMap;
use std::io::ErrorKind;

/// Lines captured per agent pane for classification (Fleet's scrape window).
pub(crate) const SCRAPE_LINES: u32 = 50;

const NO_SERVER_MARKERS: [&str; 3] = [
    "no server running",
    "error connecting to",
    "failed to connect to server",
];

/// The node's tmux servers, each behind an explicit endpoint. This is the adapter that feeds
/// [`crate::NodeCore`] and carries out [`Control`] requests.
#[derive(Default)]
pub struct TmuxServers {
    servers: BTreeMap<ServerId, Tmux<Box<dyn TmuxRunner + Send + Sync>>>,
}

impl TmuxServers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, server: ServerId, runner: Box<dyn TmuxRunner + Send + Sync>) {
        self.servers.insert(server, Tmux::with_runner(runner));
    }

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

    fn tmux(
        &self,
        server: &ServerId,
    ) -> Result<&Tmux<Box<dyn TmuxRunner + Send + Sync>>, ControlError> {
        self.servers.get(server).ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::InvalidRequest,
                format!("no tmux server {server}"),
            )
        })
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
            let agent = detect_agent(&info.current_command)?;
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

fn failed(e: TmuxError) -> ControlError {
    let kind = match &e {
        TmuxError::Spawn(_) => ErrorKindCode::TmuxUnavailable,
        TmuxError::Failed { stderr, .. }
            if NO_SERVER_MARKERS
                .iter()
                .any(|m| stderr.to_lowercase().contains(m)) =>
        {
            ErrorKindCode::TmuxServerUnavailable
        }
        _ => ErrorKindCode::RemoteCommandFailed,
    };
    ControlError::new(kind, e.to_string())
}

impl Control for TmuxServers {
    fn capture(
        &self,
        server: &ServerId,
        pane: &PaneId,
        lines: u32,
    ) -> Result<String, ControlError> {
        self.tmux(server)?
            .capture_pane(pane.as_str(), false, Some(lines))
            .map_err(failed)
    }

    fn kill_pane(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        let tmux = self.tmux(server)?;
        // The request was issued against one process; only kill the pane if it still is it.
        let current = tmux
            .list_panes()
            .map_err(failed)?
            .into_iter()
            .find(|p| p.pane_id == pane.as_str());
        match current {
            Some(p) if p.pane_pid == expected_pid => tmux.kill_pane(pane.as_str()).map_err(failed),
            _ => Err(ControlError::new(
                ErrorKindCode::UnknownPane,
                format!("pane {pane} is no longer the process this request targeted"),
            )),
        }
    }

    fn create_session(
        &self,
        server: &ServerId,
        name: &str,
        dir: &str,
        command: &str,
    ) -> Result<(), ControlError> {
        let tmux = self.tmux(server)?;
        let result = if command.is_empty() {
            tmux.new_session(name, dir)
        } else {
            tmux.new_session_running(name, dir, command)
        };
        result.map_err(failed)
    }
}
