// SPDX-License-Identifier: MIT

use crate::terminal::TerminalEnd;
use flight_ui::SurfaceChoice;

/// How a session ended and what is worth telling the user besides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOutcome {
    pub end: TerminalEnd,
    /// The surface that was last on screen, if any was.
    pub shown: Option<SurfaceChoice>,
    /// Bytes the user typed that were never delivered because their surface could not be
    /// reached or the session ended first. Counted, not hidden.
    pub undelivered: usize,
}
