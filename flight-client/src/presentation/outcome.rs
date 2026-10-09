// SPDX-License-Identifier: MIT

use crate::terminal::TerminalEnd;
use flight_present::Layout;

/// How a presentation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationOutcome {
    pub end: TerminalEnd,
    /// The layout as the user left it, to save and show again next time.
    pub layout: Layout,
    /// Bytes typed for a surface that could not be reached or was closed before they arrived.
    /// Counted, not hidden.
    pub undelivered: usize,
}
