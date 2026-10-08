// SPDX-License-Identifier: MIT

/// What a new session runs: a closed set, never a command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    /// The `claude` found on the node's own `PATH`.
    Claude,
    /// The node's normal shell: tmux's `default-shell`, started the way tmux starts it for a
    /// session created by hand.
    Shell,
}

/// A validated request to create a session, detached from the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    pub name: String,
    /// Absolute, or `~`-relative to the node's home directory.
    pub dir: String,
    pub program: Program,
}
