// SPDX-License-Identifier: MIT

use super::CreateFailure;
use crate::snapshot::{PanePreview, PaneView, UiSnapshot};
use crate::{NewSessionRequest, NewSurfaceRequest, SavedActionRequest};
use flight_state::PaneRef;

/// Where the dashboard's data comes from. The UI neither knows nor cares whether it is
/// reading tmux directly (the local/SSH [`crate::Collector`]) or an orchestrator that may be
/// on this machine, on the LAN, or hosted: it only sees snapshots, previews and a switch.
pub trait Backend: Send {
    /// The current state of everything watched.
    fn snapshot(&mut self, now: u64) -> UiSnapshot;
    /// The recent screen of one pane.
    fn preview(&mut self, pane: &PaneRef) -> PanePreview;
    /// Take the user to a pane. Backends that cannot say why not.
    fn switch_to(&mut self, pane: &PaneView) -> Result<(), String>;
    /// Add a surface (a shell) to a workspace that exists, on the workspace's own host. A
    /// backend that cannot says so.
    fn create_surface(&mut self, _request: &NewSurfaceRequest) -> Result<(), CreateFailure> {
        Err(CreateFailure::Unsupported)
    }
    /// Act on a workspace a node has saved. A backend that cannot says so.
    fn saved_action(&mut self, _request: &SavedActionRequest) -> Result<(), CreateFailure> {
        Err(CreateFailure::Unsupported)
    }
    /// Create a workspace on a node (its agent session). A backend that cannot says so.
    fn create_session(&mut self, _request: &NewSessionRequest) -> Result<(), CreateFailure> {
        Err(CreateFailure::Unsupported)
    }
}
