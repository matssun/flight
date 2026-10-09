// SPDX-License-Identifier: MIT

use crate::round::raw_placement;
use crate::session_create::{Program, SessionRequest};
use crate::{pane_agent, TmuxServers};
use flight_classify::AgentKind;
use flight_state::{HostId, SurfaceRole};
use flight_tmux::ConfigMark;
use flight_workspaces::{
    ConfigKey, ExecError, Executor, HostView, ObservedSurface, ObservedWorkspace, Observer,
    Started, SurfaceKind, WorkspaceDefinition,
};
use std::collections::BTreeMap;

/// This node's tmux servers, seen as `flight-workspaces` sees a host: something to ask what is
/// running, and something that starts a workspace or surface from a definition. Both only ever
/// use the node's existing creation paths, which never create or alter a directory.
pub(crate) struct NodeBackend<'a> {
    servers: &'a TmuxServers,
    host: HostId,
}

impl<'a> NodeBackend<'a> {
    pub(crate) fn new(servers: &'a TmuxServers, host: &str) -> Self {
        Self {
            servers,
            host: HostId::new(host),
        }
    }
}

fn key(text: &str) -> Option<ConfigKey> {
    ConfigKey::parse(text)
}

impl Observer for NodeBackend<'_> {
    /// A node asks only itself. A server that is not running contributes nothing (that is what
    /// a lost tmux server looks like); any other failure makes the whole host unreadable
    /// rather than reporting an empty one, because "nothing running" would start duplicates.
    fn observe(&self, _host: &str) -> HostView {
        let panes = match self.servers.published_panes() {
            Ok(p) => p,
            Err(reason) => return HostView::Unreachable { reason },
        };
        let mut by_workspace: BTreeMap<String, ObservedWorkspace> = BTreeMap::new();
        for (server, info) in panes {
            let placement = raw_placement(&info);
            let id = placement.workspace(&self.host, &server).to_string();
            let runs_agent = pane_agent(&info).is_some_and(|a| a != AgentKind::Other);
            let kind = match placement.role(runs_agent) {
                SurfaceRole::Agent => SurfaceKind::Agent,
                SurfaceRole::Shell => SurfaceKind::Shell,
            };
            let surface = ObservedSurface {
                surface_id: placement.surface(&self.host, &server).to_string(),
                config_key: key(&info.config_surface),
                kind,
            };
            by_workspace
                .entry(id.clone())
                .or_insert_with(|| ObservedWorkspace {
                    workspace_id: id,
                    config_key: key(&info.config_key),
                    root: placement.root(&info.current_path).to_owned(),
                    surfaces: Vec::new(),
                })
                .surfaces
                .push(surface);
        }
        HostView::Reachable(by_workspace.into_values().collect())
    }
}

impl Executor for NodeBackend<'_> {
    fn start_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError> {
        let agent = def.surfaces.iter().find(|s| s.kind == SurfaceKind::Agent);
        let first = agent
            .or_else(|| def.surfaces.first())
            .ok_or_else(|| ExecError::Refused("the saved workspace has no surfaces".to_owned()))?;
        let program = match (
            first.kind,
            first.provider.as_deref(),
            first.skip_permissions,
        ) {
            (SurfaceKind::Shell, ..) => Program::Shell,
            (SurfaceKind::Agent, Some("claude"), false) => Program::Claude,
            (SurfaceKind::Agent, Some("claude"), true) => Program::ClaudeSkipPermissions,
            (SurfaceKind::Agent, other, _) => {
                return Err(ExecError::Refused(format!(
                    "no agent provider {other:?} on this node"
                )))
            }
        };
        let request = SessionRequest {
            name: def.name.clone(),
            dir: def.root.path.clone(),
            program,
        };
        let config = ConfigMark {
            workspace: def.key.to_string(),
            surface: first.key.to_string(),
        };
        let (_server, mark) = self
            .servers
            .create_session_marked(&request, Some(config))
            .map_err(|e| ExecError::Refused(e.message))?;
        // The companion shell is a second step; if it fails the workspace is running and the
        // next pass sees one surface missing and tries only that.
        if let Some(shell) = def
            .surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Shell && s.key != first.key)
        {
            let _ = self.start_surface(def, &shell.key, SurfaceKind::Shell, &mark.workspace_id);
        }
        Ok(Started {
            workspace_id: mark.workspace_id,
        })
    }

    fn start_surface(
        &mut self,
        def: &WorkspaceDefinition,
        surface: &ConfigKey,
        kind: SurfaceKind,
        workspace_id: &str,
    ) -> Result<(), ExecError> {
        if kind != SurfaceKind::Shell {
            return Err(ExecError::Refused(
                "an agent is created with its workspace, not added to one".to_owned(),
            ));
        }
        let _ = def;
        self.servers
            .create_shell_marked(&self.host, workspace_id, surface)
            .map(|_| ())
            .map_err(|e| ExecError::Refused(e.message))
    }

    fn resume_agent(
        &mut self,
        _def: &WorkspaceDefinition,
        _surface: &ConfigKey,
        _workspace_id: &str,
    ) -> Result<(), ExecError> {
        Err(ExecError::Refused(
            "no agent provider on this node can resume a session".to_owned(),
        ))
    }
}
