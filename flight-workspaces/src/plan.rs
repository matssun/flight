// SPDX-License-Identifier: MIT

use crate::{ConfigKey, RootCheck, SurfaceKind};

/// One thing recovery may do. The set is closed and has no filesystem member: nothing here
/// creates a directory, clones, creates or repairs a worktree, or deletes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Remember which running workspace realizes a saved one (a write to the saved config only).
    Bind {
        key: ConfigKey,
        workspace_id: String,
    },
    /// Start a workspace from its definition: a replacement process, not the old one.
    StartWorkspace { key: ConfigKey },
    /// Start one missing surface of a running workspace: a replacement process.
    StartSurface {
        key: ConfigKey,
        surface: ConfigKey,
        kind: SurfaceKind,
        workspace_id: String,
    },
    /// Continue an agent's earlier session where the provider supports it.
    ResumeAgent {
        key: ConfigKey,
        surface: ConfigKey,
        workspace_id: String,
    },
}

/// Why a workspace is not in a state recovery can act on. Each carries what the user needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    HostUnreachable(String),
    Root(RootCheck),
    /// More than one running workspace could be this one; Flight does not choose.
    Ambiguous(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Running with every saved surface.
    Running,
    /// Running, with some saved surfaces absent.
    Partial,
    /// Not running; its root is usable, so it could be started.
    Stopped,
    Blocked(Blocker),
}

/// Why something that could be started was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    PolicyDoesNotStart,
    ImportedNotTrusted,
    SkipPermissionsNotTrusted,
    RootNotVerified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: ConfigKey,
    /// The running workspace that realizes it, when one does.
    pub runtime: Option<String>,
    pub health: Health,
    /// How the root compared, whether or not a process is running.
    pub root: RootCheck,
    pub actions: Vec<Action>,
    pub refusals: Vec<Refusal>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<Item>,
    /// Running workspaces no saved definition accounts for, by runtime id.
    pub unsaved: Vec<String>,
}
