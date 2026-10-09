// SPDX-License-Identifier: MIT

use super::{HostHealth, WorkspaceKey};
use flight_state::{HostId, WorkspaceId};

/// How a saved workspace stands, as its node reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedHealth {
    Running,
    /// Running with some saved surfaces absent.
    Partial,
    /// Not running, and could be started.
    Stopped,
    /// Cannot be acted on; see the root state and detail.
    Blocked,
}

/// What looking at the saved root found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedRoot {
    Verified,
    FirstSighting,
    Missing,
    NotADirectory,
    PermissionDenied,
    Unverified,
    /// Present, but not demonstrably the directory that was saved.
    Changed,
}

/// What starting a saved workspace again does about its agent's conversation, as its node says
/// (ADR-010). The dashboard never claims a continuation the node did not report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedResume {
    /// The node does not say (one that predates it): starting it again is a replacement.
    Unknown,
    /// No session was saved: starting it again is a new conversation.
    New,
    /// The earlier conversation is there and starting it again continues it.
    Continues,
    /// A session was saved but cannot be continued now: starting it again is refused, and a new
    /// conversation is a separate choice.
    CannotContinue(String),
    /// The agent's provider has no reliable way to continue a session.
    Unsupported(String),
}

/// A workspace a node has saved: shown whether or not anything of it is running, so it never
/// disappears with its processes (ADR-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedView {
    pub host: HostId,
    pub host_label: String,
    /// The node's connection. Anything but `Online` means the health below is last-known, or
    /// that the host cannot be asked at all.
    pub host_health: HostHealth,
    /// The stable saved identity; what a user action refers to.
    pub config_key: String,
    pub name: String,
    pub root: String,
    pub health: SavedHealth,
    pub root_state: SavedRoot,
    /// Failure information from the node, plain text.
    pub detail: String,
    /// The running workspace that realizes it, when one does.
    pub running: Option<String>,
    pub imported: bool,
    pub resume: SavedResume,
}

impl SavedView {
    /// How the dashboard points at it. A saved identity (`c-…`) never equals a running
    /// workspace's (`w-…`), so one key type serves both without confusing them.
    pub fn key(&self) -> WorkspaceKey {
        WorkspaceKey {
            host: self.host.clone(),
            workspace: WorkspaceId::new(&self.config_key),
        }
    }

    /// Whether it is listed as unavailable: not running. A running one is a workspace already.
    pub fn is_unavailable(&self) -> bool {
        matches!(self.health, SavedHealth::Stopped | SavedHealth::Blocked)
    }
}
