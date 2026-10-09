// SPDX-License-Identifier: MIT

use crate::SurfaceMark;

/// How the first pane of a new session starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// Whatever tmux starts in a pane by default: the server's `default-shell`, as a login
    /// shell, exactly as for a session created by hand.
    DefaultShell,
    /// One program, run directly (no shell parses it); `argv[0]` should be an absolute path.
    /// Its `PATH` is that of the tmux client, which is the calling process: tmux gives a new
    /// session the client's `PATH`, not the server's.
    Program { argv: Vec<String> },
}

/// A detached session to create. The name is matched exactly, never as a prefix. With a `mark`
/// the session is a Flight workspace whose first window is the marked surface; without one it is
/// an unmarked (legacy) session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub name: String,
    pub dir: String,
    pub launch: Launch,
    pub mark: Option<SurfaceMark>,
}
