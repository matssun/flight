// SPDX-License-Identifier: MIT

use super::{PaneView, Surface, SurfaceKind, WorkspaceKey};
use flight_state::{needs_attention, AgentState, HostId, WorkspaceId};

/// The user's project/work context on one host: the unit the dashboard lists. Its surfaces
/// (an agent, a shell) are resources attached to it; no provider such as Claude or Codex
/// defines it. Built from what the nodes publish, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// What it is called; not its identity.
    pub name: String,
    pub host: HostId,
    pub host_label: String,
    /// The directory every surface starts in.
    pub root: String,
    pub surfaces: Vec<Surface>,
}

impl Workspace {
    pub fn key(&self) -> WorkspaceKey {
        WorkspaceKey {
            host: self.host.clone(),
            workspace: self.id.clone(),
        }
    }

    /// The workspace's agent surface, if it has one running.
    pub fn agent(&self) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.kind.is_agent())
    }

    pub fn shell(&self) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.kind == SurfaceKind::Shell)
    }

    /// The surface the cursor stands on: the agent, or the shell of a workspace with none.
    pub fn anchor(&self) -> Option<&Surface> {
        self.agent().or_else(|| self.shell())
    }

    pub fn anchor_pane(&self) -> Option<&PaneView> {
        self.anchor().map(|s| &s.pane)
    }

    /// The state that orders the workspace in the list: its agent's. A shell never asks for
    /// the user, so a workspace without an agent is only ever as urgent as a shell.
    pub fn state(&self) -> AgentState {
        self.anchor().map_or(AgentState::Down, |s| s.pane.state)
    }

    /// Whether the workspace needs the user, which only its agent can say.
    pub fn needs_attention(&self) -> bool {
        self.agent().is_some_and(|s| needs_attention(s.pane.state))
    }
}
