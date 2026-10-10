// SPDX-License-Identifier: MIT

use flight_state::SurfaceId;
use flight_ui::SurfaceChoice;

/// What a surface named in a layout is, for the host that attaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// What kind of surface it is.
    pub choice: SurfaceChoice,
    /// Which one, when the workspace has more than one of that kind: its own id. `None` for the
    /// workspace's agent and shell, which are found by kind each time they are attached (the
    /// agent's window may have been replaced since).
    pub surface: Option<SurfaceId>,
}
