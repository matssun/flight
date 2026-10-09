// SPDX-License-Identifier: MIT
#![allow(dead_code)]

use flight_workspaces::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn def(name: &str, host: &str, root: &str) -> WorkspaceDefinition {
    WorkspaceDefinition {
        key: ConfigKey::mint().unwrap(),
        name: name.to_owned(),
        host: host.to_owned(),
        root: RootSpec::new(root),
        surfaces: vec![surface(SurfaceKind::Agent), surface(SurfaceKind::Shell)],
        origin: Origin::Local,
        last_workspace_id: None,
    }
}

pub fn surface(kind: SurfaceKind) -> SurfaceSpec {
    SurfaceSpec {
        key: ConfigKey::mint().unwrap(),
        kind,
        provider: (kind == SurfaceKind::Agent).then(|| "claude".to_owned()),
        skip_permissions: false,
        last_surface_id: None,
    }
}

pub fn doc_with(defs: Vec<WorkspaceDefinition>) -> Document {
    let mut doc = Document::default();
    doc.active_mut().unwrap().workspaces = defs;
    doc
}

pub fn present(ino: u64) -> RootState {
    RootState::Present {
        identity: RootIdentity { dev: 1, ino },
        git: GitMarker::Absent,
    }
}

/// Answers root probes from a table; anything unlisted is a confirmed absence.
#[derive(Default)]
pub struct Roots(pub RefCell<HashMap<String, RootState>>);

impl Roots {
    pub fn set(&self, path: &str, state: RootState) {
        self.0.borrow_mut().insert(path.to_owned(), state);
    }
}

impl RootProbe for Roots {
    fn probe(&self, _host: &str, path: &str) -> RootState {
        self.0
            .borrow()
            .get(path)
            .cloned()
            .unwrap_or(RootState::Missing)
    }
}

#[derive(Default)]
pub struct World {
    pub up: HashMap<String, bool>,
    pub running: HashMap<String, Vec<ObservedWorkspace>>,
    pub started: Vec<String>,
    pub next: u32,
    /// The next start takes effect but its reply is lost.
    pub lose_reply: bool,
}

#[derive(Clone, Default)]
pub struct Fake(pub Rc<RefCell<World>>);

impl Fake {
    pub fn host_up(&self, host: &str, up: bool) {
        self.0.borrow_mut().up.insert(host.to_owned(), up);
    }

    pub fn running(&self, host: &str) -> usize {
        self.0.borrow().running.get(host).map_or(0, Vec::len)
    }
}

impl Observer for Fake {
    fn observe(&self, host: &str) -> HostView {
        let w = self.0.borrow();
        if w.up.get(host).copied().unwrap_or(true) {
            HostView::Reachable(w.running.get(host).cloned().unwrap_or_default())
        } else {
            HostView::Unreachable {
                reason: "no route".to_owned(),
            }
        }
    }
}

impl Executor for Fake {
    fn start_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError> {
        let mut w = self.0.borrow_mut();
        w.next += 1;
        let id = format!("w-{}", w.next);
        let surfaces = def
            .surfaces
            .iter()
            .map(|s| ObservedSurface {
                surface_id: format!("s-{}-{}", w.next, s.key),
                config_key: Some(s.key.clone()),
                kind: s.kind,
            })
            .collect();
        w.running
            .entry(def.host.clone())
            .or_default()
            .push(ObservedWorkspace {
                workspace_id: id.clone(),
                config_key: Some(def.key.clone()),
                root: def.root.path.clone(),
                surfaces,
            });
        w.started.push(def.name.clone());
        if std::mem::take(&mut w.lose_reply) {
            return Err(ExecError::Unknown("timed out".to_owned()));
        }
        Ok(Started { workspace_id: id })
    }

    fn start_surface(
        &mut self,
        def: &WorkspaceDefinition,
        surface: &ConfigKey,
        kind: SurfaceKind,
        workspace_id: &str,
    ) -> Result<(), ExecError> {
        let mut w = self.0.borrow_mut();
        let list = w.running.entry(def.host.clone()).or_default();
        let ws = list
            .iter_mut()
            .find(|o| o.workspace_id == workspace_id)
            .ok_or_else(|| ExecError::Refused("gone".to_owned()))?;
        ws.surfaces.push(ObservedSurface {
            surface_id: format!("s-new-{surface}"),
            config_key: Some(surface.clone()),
            kind,
        });
        w.started.push(format!("{}:{kind:?}", def.name));
        Ok(())
    }

    fn resume_agent(
        &mut self,
        _: &WorkspaceDefinition,
        _: &ConfigKey,
        _: &str,
    ) -> Result<(), ExecError> {
        Err(ExecError::Refused("not supported".to_owned()))
    }
}
