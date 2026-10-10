// SPDX-License-Identifier: MIT

use crate::presentation::PresentationOutcome;
use crate::session::SessionOutcome;
use crate::terminal::{run_presentation, run_session, SessionRequest, TerminalHold};
use crate::OrchestratedBackend;
use flight_present::Layout;
use flight_ui::WorkspaceKey;

/// The two ways the user's terminal can be given to a workspace. Each takes the terminal over,
/// runs until the user is done or nothing can be shown, and gives it back. The real one drives
/// the user's tty; a test supplies one that answers.
pub trait Terminals {
    /// What a session keeps for the presentation that follows it: the terminal itself, still
    /// ours, and the surface that was on screen, still attached.
    type Held;

    /// One surface, full screen. When it ends because the user asked for the surfaces side by
    /// side (and only then) it returns what it kept.
    fn session(&self, request: SessionRequest) -> (SessionOutcome, Option<Self::Held>);

    /// Several surfaces at once, arranged by `layout`, carrying on from `held` if there is one.
    fn presentation(
        &self,
        workspace: WorkspaceKey,
        layout: Layout,
        held: Option<Self::Held>,
    ) -> PresentationOutcome;
}

impl Terminals for OrchestratedBackend {
    type Held = TerminalHold;

    fn session(&self, request: SessionRequest) -> (SessionOutcome, Option<TerminalHold>) {
        run_session(self, request)
    }

    fn presentation(
        &self,
        workspace: WorkspaceKey,
        layout: Layout,
        held: Option<TerminalHold>,
    ) -> PresentationOutcome {
        run_presentation(self, workspace, layout, held)
    }
}
