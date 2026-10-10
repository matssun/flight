// SPDX-License-Identifier: MIT

//! Creating workspaces and surfaces, and handing what was created to persistence (ADR-008).

use super::{failed, TmuxServers};
use crate::agent_resume::{AgentLaunch, AgentSession};
use crate::round::raw_placement;
use crate::session_create::{create, new_id};
use crate::{pane_agent, ControlError, SessionRequest, SurfaceRequest};
use flight_proto::ErrorKindCode;
use flight_state::{HostId, ServerId, SurfaceRole, WorkspaceId};
use flight_tmux::{ConfigMark, CreateError, Launch, SurfaceMark, SurfaceTag};
use flight_workspaces::ConfigKey;
use std::sync::atomic::Ordering;

impl TmuxServers {
    /// Create a workspace, optionally marked with the saved definition it realizes. Returns the
    /// server it lives on and the mark of its first surface.
    pub(super) fn session_marked(
        &self,
        request: &SessionRequest,
        config: Option<ConfigMark>,
        agent: &AgentLaunch,
    ) -> Result<(ServerId, SurfaceMark, Option<AgentSession>), ControlError> {
        // The node, not the caller, decides where a session lives: its first backend.
        let server = self.servers.keys().next().ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::TmuxUnavailable,
                "this node has no session backend",
            )
        })?;
        let tmux = self.tmux(server)?;
        let (mark, session) = create(tmux, request, &self.session_env, config, agent, failed)?;
        Ok((server.clone(), mark, session))
    }

    /// Add the companion shell to a running workspace, marked with the saved surface it
    /// realizes. The directory is the workspace's own, read live, and must still exist: tmux
    /// would otherwise quietly start the shell somewhere else. Returns the new mark and the
    /// saved key of the workspace, if it has one.
    pub(super) fn shell_marked(
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

    pub(super) fn session_create(&self, request: &SessionRequest) -> Result<(), ControlError> {
        let config = ConfigMark {
            workspace: mint_key()?.to_string(),
            surface: mint_key()?.to_string(),
        };
        let (_server, mark, session) =
            self.session_marked(request, Some(config.clone()), &AgentLaunch::New)?;
        if let Some(p) = &self.persistence {
            log_unsaved(p.record_session(request, &mark, &config, session.as_ref()));
        }
        Ok(())
    }

    pub(super) fn saved_do(&self, request: &crate::SavedActionRequest) -> Result<(), ControlError> {
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

    pub(super) fn surface_create(&self, request: &SurfaceRequest) -> Result<(), ControlError> {
        let key = mint_key()?;
        let (mark, workspace_config) =
            self.shell_marked(&request.host, request.workspace_id.as_str(), &key)?;
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
