// SPDX-License-Identifier: MIT

use crate::agent_resume::{AgentLaunch, AgentSession};
use crate::{ControlError, SessionRequest};
use flight_state::{HostId, ServerId};
use flight_tmux::{ConfigMark, PaneInfo, SurfaceMark};
use flight_workspaces::ConfigKey;

/// What the saved workspaces need of the node's tmux servers: which panes Flight publishes, and
/// the two ways of creating something. Persistence owns this interface, and `TmuxServers`
/// implements it, so the persistence code never names `TmuxServers` and a test can drive it with
/// a node that has no tmux.
pub(crate) trait NodeTmux {
    /// The panes the node publishes (those of Flight's sessions and those running an agent),
    /// from every server. A server with no tmux running contributes none; any other failure is
    /// an error, never an empty answer.
    fn published_panes(&self) -> Result<Vec<(ServerId, PaneInfo)>, String>;

    /// Create a workspace, optionally marked with the saved definition it realizes. Returns the
    /// server it lives on and the mark of its first surface.
    fn create_session_marked(
        &self,
        request: &SessionRequest,
        config: Option<ConfigMark>,
        agent: &AgentLaunch,
    ) -> Result<(ServerId, SurfaceMark, Option<AgentSession>), ControlError>;

    /// Add the companion shell to a running workspace, marked with the saved surface it
    /// realizes. Returns the new mark and the saved key of the workspace, if it has one.
    fn create_shell_marked(
        &self,
        host: &HostId,
        workspace_id: &str,
        surface: &ConfigKey,
    ) -> Result<(SurfaceMark, String), ControlError>;
}
