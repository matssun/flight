// SPDX-License-Identifier: MIT

use crate::agent_resume::{AgentLaunch, AgentSession, Claude};
use crate::round::raw_placement;
use crate::session_create::{Program, SessionRequest};
use crate::{pane_agent, TmuxServers};
use flight_classify::AgentKind;
use flight_state::{HostId, SurfaceRole};
use flight_tmux::ConfigMark;
use flight_workspaces::{
    ConfigKey, ExecError, Executor, HostView, ObservedSurface, ObservedWorkspace, Observer,
    ResumeRef, ResumeScope, ResumeStore, Started, SurfaceKind, WorkspaceDefinition,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What a node needs to continue agents' sessions: the references it holds, who it runs as and
/// where the provider keeps its data.
pub(crate) struct ResumeContext<'a> {
    pub store: Option<&'a ResumeStore>,
    pub user: String,
    pub home: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
}

/// A reference made while a workspace was started: the new session of a replacement agent.
pub(crate) struct NewReference {
    pub workspace: ConfigKey,
    pub surface: ConfigKey,
    pub reference: ResumeRef,
}

/// This node's tmux servers, seen as `flight-workspaces` sees a host: something to ask what is
/// running, and something that starts a workspace or surface from a definition. Both only ever
/// use the node's existing creation paths, which never create or alter a directory.
pub(crate) struct NodeBackend<'a> {
    servers: &'a TmuxServers,
    host: HostId,
    resume: Option<ResumeContext<'a>>,
    /// References for the agents this backend started, for the caller to keep.
    pub(crate) new_references: Vec<NewReference>,
}

impl<'a> NodeBackend<'a> {
    pub(crate) fn new(servers: &'a TmuxServers, host: &str) -> Self {
        Self {
            servers,
            host: HostId::new(host),
            resume: None,
            new_references: Vec::new(),
        }
    }

    pub(crate) fn with_resume(mut self, resume: ResumeContext<'a>) -> Self {
        self.resume = Some(resume);
        self
    }

    /// The scope a reference for `root` is good in, from this node's point of view.
    fn scope(&self, root: &str) -> Option<ResumeScope> {
        let resume = self.resume.as_ref()?;
        let canonical = canonical_root(root, resume.home.as_deref())?;
        Some(ResumeScope {
            host: self.host.to_string(),
            root: canonical,
            user: resume.user.clone(),
        })
    }

    fn remember(&mut self, def: &WorkspaceDefinition, surface: &ConfigKey, session: &AgentSession) {
        let Some(scope) = self.scope(&def.root.path) else {
            return;
        };
        if let Some(reference) = ResumeRef::new(session.provider, &session.token, scope) {
            self.new_references.push(NewReference {
                workspace: def.key.clone(),
                surface: surface.clone(),
                reference,
            });
        }
    }
}

/// A root as the provider sees it: `~` expanded, symlinks resolved. `None` when it is not there.
pub(crate) fn canonical_root(root: &str, home: Option<&std::path::Path>) -> Option<String> {
    let path = match root.strip_prefix('~') {
        Some(rest) => home?.join(rest.trim_start_matches('/')),
        None => PathBuf::from(root),
    };
    std::fs::canonicalize(path)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
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

impl NodeBackend<'_> {
    /// Start the workspace. `resume` is an earlier session already checked to be continuable;
    /// without it the agent is a new one.
    fn start_with(
        &mut self,
        def: &WorkspaceDefinition,
        resume: Option<AgentSession>,
    ) -> Result<Started, ExecError> {
        let launch = match &resume {
            Some(session) => AgentLaunch::Resume(session.clone()),
            None => AgentLaunch::New,
        };
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
        let (_server, mark, session) = self
            .servers
            .create_session_marked(&request, Some(config), &launch)
            .map_err(|e| ExecError::Refused(e.message))?;
        // A replacement is a new session; its reference replaces the old one.
        if let (Some(session), None, true) = (session, &resume, first.kind == SurfaceKind::Agent) {
            self.remember(def, &first.key.clone(), &session);
        }
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
}

impl Executor for NodeBackend<'_> {
    fn start_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError> {
        self.start_with(def, None)
    }

    fn resume_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError> {
        let cannot = |why: String| {
            ExecError::Refused(format!(
                "cannot continue the earlier conversation ({why}); nothing was started and the \
                 saved workspace was kept"
            ))
        };
        let spec = def
            .surfaces
            .iter()
            .find(|s| s.kind == SurfaceKind::Agent)
            .ok_or_else(|| cannot("this workspace has no agent".to_owned()))?;
        let resume = self
            .resume
            .as_ref()
            .ok_or_else(|| cannot("this node is not keeping session references".to_owned()))?;
        let reference = resume
            .store
            .and_then(|store| store.get(&def.key, &spec.key))
            .ok_or_else(|| cannot("no session was saved for it".to_owned()))?;
        let scope = self
            .scope(&def.root.path)
            .ok_or_else(|| cannot("the directory cannot be resolved".to_owned()))?;
        let config = resume.config_dir.clone();
        let session = Claude::check(reference, &scope, config.as_deref()).map_err(cannot)?;
        self.start_with(def, Some(session))
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
