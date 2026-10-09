// SPDX-License-Identifier: MIT

use crate::switch::AttachCommand;
use flight_ui::{SurfaceChoice, WorkspaceKey};

/// Which surface of which workspace a terminal shows, so that leaving it can mean "the other
/// surface of this workspace".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownSurface {
    pub workspace: WorkspaceKey,
    pub choice: SurfaceChoice,
}

/// What takes over the terminal once the dashboard has exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// Replace this process with a local tmux attach.
    Attach(AttachCommand),
    /// Show a remote pane through the terminal stream with this id, then come back.
    Terminal { id: Vec<u8>, shown: ShownSurface },
}
