// SPDX-License-Identifier: MIT

use crate::{ConfigKey, SurfaceKind};

/// A surface seen running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedSurface {
    pub surface_id: String,
    /// The saved surface this one was started for, when it was started from a definition.
    pub config_key: Option<ConfigKey>,
    pub kind: SurfaceKind,
}

/// A workspace seen running on a host, as the node publishes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedWorkspace {
    pub workspace_id: String,
    /// The saved workspace it was started for. Written into the backend when Flight starts a
    /// workspace from a definition, so a lost reply still leaves a mark the next pass finds.
    pub config_key: Option<ConfigKey>,
    pub root: String,
    pub surfaces: Vec<ObservedSurface>,
}

/// What asking a host for its running workspaces produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostView {
    Reachable(Vec<ObservedWorkspace>),
    Unreachable { reason: String },
}

/// Asks hosts what is running now. Reads only.
pub trait Observer {
    fn observe(&self, host: &str) -> HostView;
}
