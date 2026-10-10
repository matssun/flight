// SPDX-License-Identifier: MIT

use crate::session::{Handover, SessionOutcome};
use crate::terminal::LocalTerminal;

/// How a session ended, and what is left of it.
pub struct SessionEnding {
    pub outcome: SessionOutcome,
    /// The surface that was on screen, still attached, when the user asked for the surfaces
    /// side by side.
    pub handover: Option<Handover>,
    /// The user's terminal, with whatever was typed and not yet read still in it.
    pub local: LocalTerminal,
}
