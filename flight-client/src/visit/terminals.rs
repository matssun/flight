// SPDX-License-Identifier: MIT

use crate::presentation::PresentationOutcome;
use crate::session::SessionOutcome;
use crate::terminal::{run_presentation, run_session, SessionRequest};
use crate::OrchestratedBackend;
use flight_present::Layout;
use flight_ui::WorkspaceKey;

/// The two ways the user's terminal can be given to a workspace. Each takes the terminal over,
/// runs until the user is done or nothing can be shown, and gives it back. The real one drives
/// the user's tty; a test supplies one that answers.
pub trait Terminals {
    /// One surface, full screen.
    fn session(&self, request: SessionRequest) -> SessionOutcome;

    /// Several surfaces at once, arranged by `layout`.
    fn presentation(&self, workspace: WorkspaceKey, layout: Layout) -> PresentationOutcome;
}

impl Terminals for OrchestratedBackend {
    fn session(&self, request: SessionRequest) -> SessionOutcome {
        run_session(self, request)
    }

    fn presentation(&self, workspace: WorkspaceKey, layout: Layout) -> PresentationOutcome {
        run_presentation(self, workspace, layout)
    }
}
