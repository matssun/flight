// SPDX-License-Identifier: MIT

/// One tmux pane, as reported by `list-panes -a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    /// e.g. `%42`.
    pub pane_id: String,
    pub session_name: String,
    pub window_name: String,
    /// e.g. `@5` — stable and server-unique, unlike the window index.
    pub window_id: String,
    pub window_index: u32,
    pub current_path: String,
    pub pane_pid: u32,
    /// Active pane of the active window of a session with an attached client.
    pub focused: bool,
    /// The pieces `focused` is made of, so a caller whose own client is attached can discount it.
    pub pane_active: bool,
    pub window_active: bool,
    pub session_attached: u32,
    /// Epoch second of the window's last activity; 0 when tmux did not say (treat as unknown).
    pub window_activity: u64,
    /// The foreground command (`#{pane_current_command}`), e.g. `zsh` or `claude`.
    pub current_command: String,
    /// The session was created by Flight (carries `@flight_session`).
    pub flight_session: bool,
    pub pane_title: String,
    /// `@flight_workspace` of the pane's session: the workspace Flight made it for. Empty for
    /// a session that predates workspaces.
    pub workspace_id: String,
    /// `@flight_surface_id` and `@flight_surface` of the pane's window; empty when unmarked.
    pub surface_id: String,
    pub surface_kind: String,
    /// Where the session was started (`#{session_path}`): the workspace's root directory.
    pub session_path: String,
    /// The session's tmux id, e.g. `$3`. Server-scoped, like a pane id.
    pub session_id: String,
    /// `@flight_config` of the session and `@flight_config_surface` of the window: the saved
    /// definitions they were started for (ADR-008). Empty when started any other way.
    pub config_key: String,
    pub config_surface: String,
}

impl PaneInfo {
    /// `focused` as if `own_clients` of the attached clients of this pane's session did not
    /// exist (a control connection of our own is not a person looking at the pane).
    pub fn focused_excluding(&self, own_clients: u32) -> bool {
        self.pane_active
            && self.window_active
            && self.session_attached.saturating_sub(own_clients) > 0
    }
}
