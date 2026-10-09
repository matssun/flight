// SPDX-License-Identifier: MIT

use crate::persistence::saved_report::{capped, saved_workspace};
use crate::persistence::NodeBackend;
use crate::{ControlError, Program, SavedAction, SavedActionRequest, SessionRequest, TmuxServers};
use flight_proto::ErrorKindCode;
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
    /// Held for as long as this node runs: no other process edits the saved file meanwhile.
    _lock: Option<flight_workspaces::StoreLock>,
}

impl WorkspacePersistence {
    /// Open the saved file in `dir`. An unreadable or newer file disables persistence for this
    /// run (see [`Self::disabled_reason`]); it is never replaced by defaults.
    pub fn open(dir: &Path, host: impl Into<String>, home: Option<std::path::PathBuf>) -> Self {
        let store = Store::new(dir);
        // One Flight process per saved file: a second node on the same directory would race
        // the first, so it runs without persistence and says why.
        let (lock, state) = match store.lock_waiting(std::time::Duration::from_secs(2)) {
            Err(e) => (None, State::Disabled(e.to_string())),
            Ok(lock) => {
                let _ = store.sweep_temporaries();
                let state = match store.load() {
                    Ok((doc, _how)) => State::Active {
                        store,
                        doc: Box::new(doc),
                    },
                    Err(e) => State::Disabled(e.to_string()),
                };
                (Some(lock), state)
            }
        };
        Self {
            host: host.into(),
            probe: FsProbe::new(home),
            state: Mutex::new(state),
            _lock: lock,
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

    /// Carry out a user's operation on one saved workspace of this node. `Retry` is handled by
    /// the caller (it only asks for an earlier report); every other action changes the saved
    /// file, or starts the workspace, and nothing here touches the filesystem outside it.
    pub fn act(
        &self,
        servers: &TmuxServers,
        request: &SavedActionRequest,
    ) -> Result<(), ControlError> {
        let key = ConfigKey::parse(&request.config_key)
            .ok_or_else(|| refuse(ErrorKindCode::InvalidRequest, "invalid key"))?;
        let unknown = || refuse(ErrorKindCode::UnknownWorkspace, "no such saved workspace");
        match &request.action {
            SavedAction::Retry => Ok(()),
            SavedAction::Remove => self.change(&key, |p, k| p.remove(k).map(|_| ())),
            SavedAction::SetRoot(path) => {
                self.change(&key, |p, k| p.set_root(k, path.clone()).then_some(()))
            }
            SavedAction::Trust => self.change(&key, |p, k| {
                p.get_mut(k)
                    .map(|w| w.origin = flight_workspaces::Origin::Local)
            }),
            SavedAction::AcceptRoot => self.accept_root(&key),
            SavedAction::Restore => self.restore(servers, &key),
        }
        .and_then(|()| {
            self.exists_or(&request.action, &key)
                .ok_or_else(unknown)
                .map(|_| ())
        })
    }

    /// Whether the action's target is (or, for a removal, was) a saved workspace here: the
    /// answer to an unknown key is the same for every action.
    fn exists_or(&self, action: &SavedAction, key: &ConfigKey) -> Option<()> {
        match (action, &*self.lock()) {
            (SavedAction::Remove | SavedAction::Retry, _) => Some(()),
            (_, State::Active { doc, .. }) => doc.active()?.get(key).map(|_| ()),
            _ => None,
        }
    }

    fn change(
        &self,
        key: &ConfigKey,
        f: impl FnOnce(&mut flight_workspaces::Profile, &ConfigKey) -> Option<()>,
    ) -> Result<(), ControlError> {
        let mut found = false;
        self.mutate(|doc| match doc.active_mut() {
            Some(profile) => {
                found = f(profile, key).is_some();
                found
            }
            None => false,
        })
        .map_err(|why| refuse(ErrorKindCode::RemoteCommandFailed, why))?;
        if found {
            Ok(())
        } else {
            Err(refuse(
                ErrorKindCode::UnknownWorkspace,
                "no such saved workspace",
            ))
        }
    }

    /// The directory now at the saved path is the one meant: remember it. Only a directory that
    /// is there can be accepted, and accepting nothing is created or changed on disk.
    fn accept_root(&self, key: &ConfigKey) -> Result<(), ControlError> {
        let path = match &*self.lock() {
            State::Active { doc, .. } => doc
                .active()
                .and_then(|p| p.get(key))
                .map(|w| w.root.path.clone()),
            State::Disabled(_) => None,
        }
        .ok_or_else(|| refuse(ErrorKindCode::UnknownWorkspace, "no such saved workspace"))?;
        let state = self.probe.probe(&self.host, &path);
        if !matches!(state, flight_workspaces::RootState::Present { .. }) {
            return Err(refuse(
                ErrorKindCode::InvalidDirectory,
                format!("{path} is not there to accept"),
            ));
        }
        self.change(key, |p, k| p.get_mut(k).map(|w| w.root.record(&state)))
    }

    /// Start one saved workspace again, as a replacement process, if its root is verified and
    /// nothing in the definition needs a permission the user has not given. Idempotent: a
    /// workspace that already runs is left alone.
    fn restore(&self, servers: &TmuxServers, key: &ConfigKey) -> Result<(), ControlError> {
        let mut guard = self.lock();
        let State::Active { store, doc } = &mut *guard else {
            return Err(refuse(
                ErrorKindCode::Unsupported,
                "workspaces are not being saved on this node",
            ));
        };
        let def = doc
            .active()
            .and_then(|p| p.get(key))
            .cloned()
            .ok_or_else(|| refuse(ErrorKindCode::UnknownWorkspace, "no such saved workspace"))?;
        // Only this workspace is considered: a scratch document holds it alone.
        let mut scratch = flight_workspaces::Document::default();
        if let Some(p) = scratch.active_mut() {
            p.workspaces.push(def);
        }
        let policy = RecoveryPolicy {
            start_missing: true,
            ..RecoveryPolicy::default()
        };
        let observer = NodeBackend::new(servers, &self.host);
        let mut executor = NodeBackend::new(servers, &self.host);
        let report = recover(
            &mut scratch,
            &[self.host.as_str()],
            &observer,
            &self.probe,
            &mut executor,
            &policy,
        );
        let learned = scratch.active().and_then(|p| p.get(key)).cloned();
        if let (Some(updated), Some(profile)) = (learned, doc.active_mut()) {
            if profile.get(key) != Some(&updated) {
                profile.upsert(updated);
                store
                    .save(doc)
                    .map_err(|e| refuse(ErrorKindCode::RemoteCommandFailed, e.to_string()))?;
            }
        }
        restore_verdict(&report)
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

fn refuse(kind: ErrorKindCode, message: impl Into<String>) -> ControlError {
    ControlError::new(kind, message)
}

/// What a restore pass amounts to for the user: done, or why it was not.
fn restore_verdict(report: &RecoveryReport) -> Result<(), ControlError> {
    use flight_workspaces::{Blocker, Health, Outcome, Refusal};
    let Some(item) = report.items.first() else {
        return Err(refuse(
            ErrorKindCode::UnknownWorkspace,
            "no such saved workspace",
        ));
    };
    if let Some(r) = item.refusals.first() {
        return Err(match r {
            Refusal::ImportedNotTrusted => refuse(
                ErrorKindCode::NotAuthorized,
                "this workspace was imported; trust it before it can start anything",
            ),
            Refusal::SkipPermissionsNotTrusted => refuse(
                ErrorKindCode::NotAuthorized,
                "this workspace runs an agent without permission prompts; create it again to allow that",
            ),
            Refusal::RootNotVerified | Refusal::PolicyDoesNotStart => refuse(
                ErrorKindCode::InvalidDirectory,
                "the directory is not verified",
            ),
        });
    }
    if let Health::Blocked(why) = &item.health {
        return Err(match why {
            Blocker::Root(_) => refuse(
                ErrorKindCode::InvalidDirectory,
                "the directory is missing, unverified or not the one that was saved",
            ),
            Blocker::Ambiguous(_) => refuse(
                ErrorKindCode::AlreadyExists,
                "more than one running workspace could be this one; not choosing",
            ),
            Blocker::HostUnreachable(why) => refuse(ErrorKindCode::NodeUnreachable, why.clone()),
        });
    }
    for (_, _, outcome) in &report.done {
        match outcome {
            Outcome::Refused(why) => {
                return Err(refuse(ErrorKindCode::RemoteCommandFailed, why.clone()))
            }
            Outcome::Unknown(why) => {
                return Err(refuse(
                    ErrorKindCode::RemoteCommandFailed,
                    format!("not sure it started ({why}); check again"),
                ))
            }
            _ => {}
        }
    }
    Ok(())
}
