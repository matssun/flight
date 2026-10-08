// SPDX-License-Identifier: MIT

use crate::switch::AttachCommand;

/// What takes over the terminal once the dashboard has exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// Replace this process with a local tmux attach.
    Attach(AttachCommand),
    /// Show a remote pane through the terminal stream with this id, then come back.
    Terminal(Vec<u8>),
}
