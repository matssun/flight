// SPDX-License-Identifier: MIT

use crate::pane_agent;
use crate::persistence::WorkspacePersistence;
use crate::round::raw_placement;
use crate::session_create::{create, new_id};
use crate::{
    tmux_attach_command, Control, ControlError, OpenedTerminal, PaneObservation, Round,
    ServerOutcome, SessionEnv, SessionRequest, SurfaceRequest, TerminalProcess, TerminalSpec,
    Unavailable,
};
use flight_proto::ErrorKindCode;
use flight_state::{HostId, PaneId, ServerId, SurfaceRole, WorkspaceId};
use flight_tmux::{
    ConfigMark, CreateError, Launch, SurfaceMark, SurfaceTag, Tmux, TmuxEndpoint, TmuxError,
    TmuxRunner,
};
use flight_workspaces::ConfigKey;
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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

    /// The panes the node publishes (those of Flight's sessions and those running an agent),
    /// from every server. A server with no tmux running contributes none; any other failure is
    /// an error, never an empty answer.
    pub(crate) fn published_panes(&self) -> Result<Vec<(ServerId, flight_tmux::PaneInfo)>, String> {
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

    /// Create a workspace, optionally marked with the saved definition it realizes. Returns the
    /// server it lives on and the mark of its first surface.
    pub(crate) fn create_session_marked(
        &self,
        request: &SessionRequest,
        config: Option<ConfigMark>,
    ) -> Result<(ServerId, SurfaceMark), ControlError> {
        // The node, not the caller, decides where a session lives: its first backend.
        let server = self.servers.keys().next().ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::TmuxUnavailable,
                "this node has no session backend",
            )
        })?;
        let tmux = self.tmux(server)?;
        let mark = create(tmux, request, &self.session_env, config, failed)?;
        Ok((server.clone(), mark))
    }

    /// Add the companion shell to a running workspace, marked with the saved surface it
    /// realizes. The directory is the workspace's own, read live, and must still exist: tmux
    /// would otherwise quietly start the shell somewhere else. Returns the new mark and the
    /// saved key of the workspace, if it has one.
    pub(crate) fn create_shell_marked(
        &self,
        host: &HostId,
        workspace_id: &str,
        surface: &ConfigKey,
    ) -> Result<(SurfaceMark, String), ControlError> {
        let _one_at_a_time = self.surfaces.lock().unwrap_or_else(|p| p.into_inner());
        let id = WorkspaceId::new(workspace_id);
        let (server, panes) = self.workspace_panes(host, &id)?;
        let tmux = self.tmux(&server)?;
        let is_shell = |p: &flight_tmux::PaneInfo| {
            let runs_agent = pane_agent(p).is_some_and(|a| a != flight_classify::AgentKind::Other);
            raw_placement(p).role(runs_agent) == SurfaceRole::Shell
        };
        if panes.iter().any(is_shell) {
            return Err(ControlError::new(
                ErrorKindCode::AlreadyExists,
                "this workspace already has a shell",
            ));
        }
        let first = panes.first().ok_or_else(|| unknown_workspace(&id))?;
        if first.session_path.is_empty() {
            return Err(ControlError::new(
                ErrorKindCode::InvalidDirectory,
                "this workspace has no known root directory",
            ));
        }
        if !std::fs::metadata(&first.session_path).is_ok_and(|m| m.is_dir()) {
            return Err(ControlError::new(
                ErrorKindCode::InvalidDirectory,
                format!("{} does not exist on this node", first.session_path),
            ));
        }
        let mark = SurfaceMark {
            workspace_id: workspace_id.to_owned(),
            surface_id: new_id('s')?,
            kind: SurfaceTag::Shell,
            config: Some(ConfigMark {
                workspace: first.config_key.clone(),
                surface: surface.to_string(),
            }),
        };
        match tmux.create_surface_window(
            &first.session_id,
            &first.session_path,
            &Launch::DefaultShell,
            &mark,
        ) {
            Ok(_window) => Ok((mark, first.config_key.clone())),
            Err(CreateError::Exited) => Err(ControlError::new(
                ErrorKindCode::ProgramUnavailable,
                "the shell exited as soon as it started; nothing was created",
            )),
            Err(CreateError::AlreadyExists) => Err(ControlError::new(
                ErrorKindCode::AlreadyExists,
                "this workspace already has a shell",
            )),
            Err(CreateError::Tmux(e)) => Err(failed(e)),
        }
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

    /// The server a workspace lives on and the panes of its surfaces, read live from the
    /// backend. The workspace is found by its id, never by a name or a backend id the caller
    /// supplied.
    fn workspace_panes(
        &self,
        host: &HostId,
        id: &WorkspaceId,
    ) -> Result<(ServerId, Vec<flight_tmux::PaneInfo>), ControlError> {
        for (server, tmux) in &self.servers {
            let Ok(all) = tmux.list_panes() else { continue };
            let panes: Vec<_> = all
                .into_iter()
                .filter(|p| raw_placement(p).workspace(host, server) == *id)
                .collect();
            if !panes.is_empty() {
                return Ok((server.clone(), panes));
            }
        }
        Err(unknown_workspace(id))
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

    fn reveal_pane(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        let tmux = self.tmux(server)?;
        let current = tmux
            .list_panes()
            .map_err(failed)?
            .into_iter()
            .find(|p| p.pane_id == pane.as_str())
            .ok_or_else(|| {
                ControlError::new(ErrorKindCode::UnknownPane, format!("no pane {pane}"))
            })?;
        // The request was issued against one process; only act if tmux still shows it.
        if current.pane_pid != expected_pid {
            return Err(ControlError::new(
                ErrorKindCode::PaneChanged,
                format!("pane {pane} is no longer the process this request targeted"),
            ));
        }
        tmux.reveal_pane(&current.window_id, pane.as_str())
            .map_err(failed)
    }

    fn open_terminal(&self, spec: &TerminalSpec) -> Result<OpenedTerminal, ControlError> {
        let endpoint = self.terminals.get(&spec.server).ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::Unsupported,
                format!("terminals are not enabled for server {}", spec.server),
            )
        })?;
        // The same check as a reveal, before any PTY exists.
        let tmux = self.tmux(&spec.server)?;
        let current = tmux
            .list_panes()
            .map_err(failed)?
            .into_iter()
            .find(|p| p.pane_id == spec.pane.as_str())
            .ok_or_else(|| {
                ControlError::new(ErrorKindCode::UnknownPane, format!("no pane {}", spec.pane))
            })?;
        if current.pane_pid != spec.pid {
            return Err(ControlError::new(
                ErrorKindCode::PaneChanged,
                format!(
                    "pane {} is no longer the process this request targeted",
                    spec.pane
                ),
            ));
        }
        // tmux repeats the check inside the command that attaches.
        let args = tmux_attach_command(endpoint, spec.pane.as_str(), spec.pid);
        let env = terminal_env(&spec.term);
        let mut opened = TerminalProcess::spawn("tmux", &args, &env, spec.cols, spec.rows)
            .map_err(|e| {
                ControlError::new(
                    ErrorKindCode::RemoteCommandFailed,
                    format!("cannot start a terminal: {e}"),
                )
            })?;
        if let Some(client_pid) = opened.process.process_id() {
            let tmux = Tmux::new(endpoint.clone());
            opened.redraw = Box::new(move || {
                let _ = tmux.refresh_client_of_pid(client_pid);
            });
        }
        Ok(opened)
    }

    fn create_session(&self, request: &SessionRequest) -> Result<(), ControlError> {
        let config = ConfigMark {
            workspace: mint_key()?.to_string(),
            surface: mint_key()?.to_string(),
        };
        let (_server, mark) = self.create_session_marked(request, Some(config.clone()))?;
        if let Some(p) = &self.persistence {
            log_unsaved(p.record_session(request, &mark, &config));
        }
        Ok(())
    }

    fn saved_action(&self, request: &crate::SavedActionRequest) -> Result<(), ControlError> {
        let persistence = self.persistence.as_ref().ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::Unsupported,
                "workspaces are not being saved on this node",
            )
        })?;
        if let Some(why) = persistence.disabled_reason() {
            return Err(ControlError::new(
                ErrorKindCode::Unsupported,
                format!("the saved workspaces file cannot be used: {why}"),
            ));
        }
        persistence.act(self, request)?;
        // Whatever was asked, the next report is the answer: bring it forward.
        self.retries.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn create_surface(&self, request: &SurfaceRequest) -> Result<(), ControlError> {
        let key = mint_key()?;
        let (mark, workspace_config) =
            self.create_shell_marked(&request.host, request.workspace_id.as_str(), &key)?;
        if let Some(p) = &self.persistence {
            log_unsaved(p.record_surface(
                &workspace_config,
                request.workspace_id.as_str(),
                &mark,
                &key,
            ));
        }
        Ok(())
    }
}

fn mint_key() -> Result<ConfigKey, ControlError> {
    ConfigKey::mint().map_err(|_| {
        ControlError::new(
            ErrorKindCode::RemoteCommandFailed,
            "this node cannot make a new id",
        )
    })
}

/// A workspace that could not be saved still exists; the next reconciliation records it.
fn log_unsaved(result: Result<(), String>) {
    if let Err(why) = result {
        eprintln!("flight-node: workspace created but not saved: {why}");
    }
}

fn unknown_workspace(id: &WorkspaceId) -> ControlError {
    ControlError::new(
        ErrorKindCode::UnknownWorkspace,
        format!("no workspace {id} on this node"),
    )
}

/// The whole environment of a terminal's tmux client: nothing is inherited but where to find
/// programs and the home directory. In particular no `TMUX` or `TMUX_PANE`.
fn terminal_env(term: &str) -> Vec<(String, String)> {
    let var = |name: &str, default: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_owned())
    };
    vec![
        (
            "PATH".to_owned(),
            var("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"),
        ),
        ("HOME".to_owned(), var("HOME", "/")),
        ("TERM".to_owned(), term.to_owned()),
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
    ]
}
