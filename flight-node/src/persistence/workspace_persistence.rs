// SPDX-License-Identifier: MIT

use crate::persistence::saved_report::{capped, saved_workspace};
use crate::persistence::NodeBackend;
use crate::{Program, SessionRequest, TmuxServers};
use flight_tmux::{ConfigMark, SurfaceMark, SurfaceTag};
use flight_workspaces::{
    recover, ConfigKey, Document, FsProbe, Observer, Origin, RecoveryPolicy, RecoveryReport,
    RootProbe, RootSpec, Store, SurfaceKind, SurfaceSpec, WorkspaceDefinition,
};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

enum State {
    Active {
        store: Store,
        doc: Box<Document>,
    },
    /// The saved file could not be used. It is left exactly as found, nothing is saved over it,
    /// and the node carries on without persistence until the user deals with the file.
    Disabled(String),
}

/// The saved workspaces of this node: loaded once, changed only under one lock, saved after
/// every change. Saving failures never fail the operation that caused them (the workspace
/// exists either way, and the next reconciliation records it again).
pub struct WorkspacePersistence {
    host: String,
    probe: FsProbe,
    state: Mutex<State>,
}

impl WorkspacePersistence {
    /// Open the saved file in `dir`. An unreadable or newer file disables persistence for this
    /// run (see [`Self::disabled_reason`]); it is never replaced by defaults.
    pub fn open(dir: &Path, host: impl Into<String>, home: Option<std::path::PathBuf>) -> Self {
        let store = Store::new(dir);
        let _ = store.sweep_temporaries();
        let state = match store.load() {
            Ok((doc, _how)) => State::Active {
                store,
                doc: Box::new(doc),
            },
            Err(e) => State::Disabled(e.to_string()),
        };
        Self {
            host: host.into(),
            probe: FsProbe::new(home),
            state: Mutex::new(state),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn disabled_reason(&self) -> Option<String> {
        match &*self.lock() {
            State::Disabled(why) => Some(why.clone()),
            State::Active { .. } => None,
        }
    }

    /// A copy of the saved document, for inspection.
    pub fn document(&self) -> Option<Document> {
        match &*self.lock() {
            State::Active { doc, .. } => Some((**doc).clone()),
            State::Disabled(_) => None,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Change the document and save it if `f` says it changed something.
    fn mutate(&self, f: impl FnOnce(&mut Document) -> bool) -> Result<(), String> {
        match &mut *self.lock() {
            State::Disabled(why) => Err(format!("workspaces are not being saved: {why}")),
            State::Active { store, doc } => {
                if f(doc) {
                    store.save(doc).map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        }
    }

    /// Remember a workspace this node just created.
    pub(crate) fn record_session(
        &self,
        request: &SessionRequest,
        mark: &SurfaceMark,
        config: &ConfigMark,
    ) -> Result<(), String> {
        let (Some(key), Some(surface)) = (
            ConfigKey::parse(&config.workspace),
            ConfigKey::parse(&config.surface),
        ) else {
            return Err("invalid configuration key".to_owned());
        };
        let mut root = RootSpec::new(request.dir.clone());
        root.record(&self.probe.probe(&self.host, &request.dir));
        let spec = SurfaceSpec {
            key: surface,
            kind: match mark.kind {
                SurfaceTag::Agent => SurfaceKind::Agent,
                SurfaceTag::Shell => SurfaceKind::Shell,
            },
            provider: (mark.kind == SurfaceTag::Agent).then(|| "claude".to_owned()),
            skip_permissions: request.program == Program::ClaudeSkipPermissions,
            last_surface_id: Some(mark.surface_id.clone()),
        };
        let def = WorkspaceDefinition {
            key,
            name: request.name.clone(),
            host: self.host.clone(),
            root,
            surfaces: vec![spec],
            origin: Origin::Local,
            last_workspace_id: Some(mark.workspace_id.clone()),
        };
        self.mutate(|doc| doc.active_mut().map(|p| p.upsert(def)).is_some())
    }

    /// Remember a surface added to a running workspace, if that workspace is a saved one.
    pub(crate) fn record_surface(
        &self,
        workspace_config: &str,
        workspace_id: &str,
        mark: &SurfaceMark,
        surface: &ConfigKey,
    ) -> Result<(), String> {
        let spec = SurfaceSpec {
            key: surface.clone(),
            kind: SurfaceKind::Shell,
            provider: None,
            skip_permissions: false,
            last_surface_id: Some(mark.surface_id.clone()),
        };
        self.mutate(|doc| {
            let Some(profile) = doc.active_mut() else {
                return false;
            };
            let found = profile.workspaces.iter_mut().find(|w| {
                w.key.as_str() == workspace_config
                    || w.last_workspace_id.as_deref() == Some(workspace_id)
            });
            match found {
                Some(w) if !w.surfaces.iter().any(|s| s.key == spec.key) => {
                    w.surfaces.push(spec);
                    true
                }
                _ => false,
            }
        })
    }

    /// Forget a saved workspace. Only the reference goes: no directory, repository or running
    /// process is touched.
    pub fn remove(&self, key: &ConfigKey) -> Result<(), String> {
        self.mutate(|doc| doc.active_mut().and_then(|p| p.remove(key)).is_some())
    }

    /// The saved workspaces and how each stands now, for the wire. Reads only: it looks at the
    /// running workspaces and at the roots and changes nothing, saved or running. Empty when
    /// persistence is disabled.
    pub fn report(&self, servers: &TmuxServers) -> Vec<flight_proto::SavedWorkspace> {
        let guard = self.lock();
        let State::Active { doc, .. } = &*guard else {
            return Vec::new();
        };
        let Some(profile) = doc.active() else {
            return Vec::new();
        };
        let backend = NodeBackend::new(servers, &self.host);
        let views = [(self.host.clone(), backend.observe(&self.host))]
            .into_iter()
            .collect();
        let plan =
            flight_workspaces::plan(profile, &views, &self.probe, &RecoveryPolicy::default());
        capped(
            plan.items
                .iter()
                .filter_map(|item| Some(saved_workspace(profile.get(&item.key)?, item)))
                .collect(),
        )
    }

    /// One reconciliation pass of the saved workspaces against this node's tmux servers. The
    /// lock is held throughout, so two passes cannot interleave. Returns `None` when
    /// persistence is disabled.
    pub fn recover(
        &self,
        servers: &TmuxServers,
        policy: &RecoveryPolicy,
    ) -> Option<Result<RecoveryReport, String>> {
        let mut guard = self.lock();
        let State::Active { store, doc } = &mut *guard else {
            return None;
        };
        let observer = NodeBackend::new(servers, &self.host);
        let mut executor = NodeBackend::new(servers, &self.host);
        let report = recover(
            doc,
            &[self.host.as_str()],
            &observer,
            &self.probe,
            &mut executor,
            policy,
        );
        if report.changed {
            if let Err(e) = store.save(doc) {
                return Some(Err(e.to_string()));
            }
        }
        Some(Ok(report))
    }
}
