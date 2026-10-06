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
    pub pane_title: String,
}
