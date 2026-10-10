// SPDX-License-Identifier: MIT

//! The node's tmux servers. One type, four jobs, one file each: this file registers servers and
//! is the [`Control`] / [`NodeTmux`] face; `observe.rs` reads them, `act.rs` acts on a pane or
//! a terminal, `create.rs` creates workspaces and surfaces and hands them to persistence.

use crate::persistence::{NodeTmux, WorkspacePersistence};
use crate::{
    Control, ControlError, OpenedTerminal, SessionEnv, SessionRequest, SurfaceRequest, TerminalSpec,
};
use flight_proto::ErrorKindCode;
use flight_state::{PaneId, ServerId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxError, TmuxRunner};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

mod act;
mod create;
mod observe;

/// Lines captured per agent pane for classification (Fleet's scrape window).
pub(crate) const SCRAPE_LINES: u32 = 50;

const NO_SERVER_MARKERS: [&str; 4] = [
    "no server running",
    "error connecting to",
    "failed to connect to server",
    // The server went away while the client was talking to it: the same fact, a moment later.
    "server exited unexpectedly",
];

/// The node's tmux servers, each behind an explicit endpoint. This is the adapter that feeds
/// [`crate::NodeCore`] and carries out [`Control`] requests.
#[derive(Default)]
pub struct TmuxServers {
    servers: BTreeMap<ServerId, Tmux<Box<dyn TmuxRunner + Send + Sync>>>,
    /// Servers a terminal may be opened on, with the endpoint its tmux client connects to.
    terminals: BTreeMap<ServerId, TmuxEndpoint>,
    /// What a created session may start from: `PATH` and `~`.
    session_env: SessionEnv,
    /// Surface creation checks "no shell yet" and then adds one: one at a time, so two
    /// requests cannot both pass the check.
    surfaces: Mutex<()>,
    /// The saved workspaces of this node, when persistence is on (ADR-008).
    persistence: Option<WorkspacePersistence>,
    /// Bumped by a user's "retry": the saved-workspace reporter looks again at once.
    retries: AtomicU64,
}

impl TmuxServers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, server: ServerId, runner: Box<dyn TmuxRunner + Send + Sync>) {
        self.servers.insert(server, Tmux::with_runner(runner));
    }

    /// Allow terminals on `server`, whose tmux client connects to `endpoint`. A server not
    /// registered here refuses to open one.
    pub fn allow_terminal(&mut self, server: ServerId, endpoint: TmuxEndpoint) {
        self.terminals.insert(server, endpoint);
    }

    /// Register a server on the local tmux socket `socket`, with terminals allowed on it, in one
    /// step: the runner and the terminal endpoint cannot be given separately, so a server cannot
    /// be registered for observing and forgotten for terminals. Returns the endpoint, for an
    /// observer that connects to the same server.
    pub fn add_local(&mut self, socket: &str) -> Result<TmuxEndpoint, TmuxError> {
        let endpoint = TmuxEndpoint::named(socket)?;
        let server = ServerId::new(socket);
        self.add(
            server.clone(),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        self.allow_terminal(server, endpoint.clone());
        Ok(endpoint)
    }

    /// Replace the environment sessions are created from (the process's, by default).
    pub fn set_session_env(&mut self, env: SessionEnv) {
        self.session_env = env;
    }

    /// Keep the workspaces created here, and let them be reconciled after a restart.
    pub fn enable_persistence(&mut self, persistence: WorkspacePersistence) {
        self.persistence = Some(persistence);
    }

    /// The saved workspaces and their health, for the wire; empty without persistence.
    pub fn saved_report(&self) -> Vec<flight_proto::SavedWorkspace> {
        self.persistence
            .as_ref()
            .map(|p| p.report(self))
            .unwrap_or_default()
    }

    /// How many times a user has asked to look again; the reporter reports sooner when it grows.
    pub fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }

    pub fn persistence(&self) -> Option<&WorkspacePersistence> {
        self.persistence.as_ref()
    }

    /// One reconciliation pass of the saved workspaces against these servers (ADR-008). `None`
    /// when persistence is off.
    pub fn recover_saved(
        &self,
        policy: &flight_workspaces::RecoveryPolicy,
    ) -> Option<Result<flight_workspaces::RecoveryReport, String>> {
        self.persistence.as_ref()?.recover(self, policy)
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
        self.pane_capture(server, pane, lines)
    }

    fn kill_pane(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        self.pane_kill(server, pane, expected_pid)
    }

    fn reveal_pane(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        self.pane_reveal(server, pane, expected_pid)
    }

    fn open_terminal(&self, spec: &TerminalSpec) -> Result<OpenedTerminal, ControlError> {
        self.terminal_open(spec)
    }

    fn create_session(&self, request: &SessionRequest) -> Result<(), ControlError> {
        self.session_create(request)
    }

    fn saved_action(&self, request: &crate::SavedActionRequest) -> Result<(), ControlError> {
        self.saved_do(request)
    }

    fn create_surface(&self, request: &SurfaceRequest) -> Result<(), ControlError> {
        self.surface_create(request)
    }
}

impl NodeTmux for TmuxServers {
    fn published_panes(&self) -> Result<Vec<(ServerId, flight_tmux::PaneInfo)>, String> {
        self.published()
    }

    fn create_session_marked(
        &self,
        request: &SessionRequest,
        config: Option<flight_tmux::ConfigMark>,
        agent: &crate::agent_resume::AgentLaunch,
    ) -> Result<
        (
            ServerId,
            flight_tmux::SurfaceMark,
            Option<crate::agent_resume::AgentSession>,
        ),
        ControlError,
    > {
        self.session_marked(request, config, agent)
    }

    fn create_shell_marked(
        &self,
        host: &flight_state::HostId,
        workspace_id: &str,
        surface: &flight_workspaces::ConfigKey,
    ) -> Result<(flight_tmux::SurfaceMark, String), ControlError> {
        self.shell_marked(host, workspace_id, surface)
    }
}
