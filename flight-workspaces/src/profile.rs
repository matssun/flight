// SPDX-License-Identifier: MIT

use crate::{ConfigKey, Origin, WorkspaceDefinition};
use serde::{Deserialize, Serialize};

/// A named arrangement of workspaces. Several can be kept; one is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default, rename = "workspace", skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceDefinition>,
    /// Runtime workspace ids the user removed from the saved set while they were still running.
    /// Without this, recording what is live would put them straight back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dismissed: Vec<String>,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            workspaces: Vec::new(),
            dismissed: Vec::new(),
        }
    }

    pub fn get(&self, key: &ConfigKey) -> Option<&WorkspaceDefinition> {
        self.workspaces.iter().find(|w| &w.key == key)
    }

    pub fn get_mut(&mut self, key: &ConfigKey) -> Option<&mut WorkspaceDefinition> {
        self.workspaces.iter_mut().find(|w| &w.key == key)
    }

    /// Add or replace by key.
    pub fn upsert(&mut self, def: WorkspaceDefinition) {
        match self.get_mut(&def.key) {
            Some(slot) => *slot = def,
            None => self.workspaces.push(def),
        }
    }

    /// Forget a workspace. Only the saved reference goes: no directory, repository, worktree or
    /// running process is touched (the function has no way to).
    pub fn remove(&mut self, key: &ConfigKey) -> Option<WorkspaceDefinition> {
        let at = self.workspaces.iter().position(|w| &w.key == key)?;
        let def = self.workspaces.remove(at);
        if let Some(id) = &def.last_workspace_id {
            if !self.dismissed.contains(id) {
                self.dismissed.push(id.clone());
            }
        }
        Some(def)
    }

    /// Change where a workspace is rooted. The identity is kept; what was recorded about the old
    /// root is dropped so the new one is verified on first sight.
    pub fn set_root(&mut self, key: &ConfigKey, path: impl Into<String>) -> bool {
        match self.get_mut(key) {
            Some(w) => {
                w.root = crate::RootSpec::new(path);
                true
            }
            None => false,
        }
    }

    /// A copy under a new name with fresh identities and no runtime hints, so it can never be
    /// mistaken for, or adopt, the processes of the original.
    pub fn duplicate(&self, name: impl Into<String>) -> Result<Self, std::io::Error> {
        let mut copy = Self::new(name);
        for w in &self.workspaces {
            let mut w = w.clone();
            w.key = ConfigKey::mint()?;
            w.last_workspace_id = None;
            for s in &mut w.surfaces {
                s.key = ConfigKey::mint()?;
                s.last_surface_id = None;
            }
            copy.workspaces.push(w);
        }
        Ok(copy)
    }

    /// Mark every definition as imported: nothing in it may start a process until the user
    /// trusts it, and nothing runtime-specific survives.
    pub fn into_imported(mut self) -> Self {
        self.dismissed.clear();
        for w in &mut self.workspaces {
            w.origin = Origin::Imported;
            w.last_workspace_id = None;
            w.root.identity = None;
            for s in &mut w.surfaces {
                s.last_surface_id = None;
            }
        }
        self
    }
}
