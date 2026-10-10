// SPDX-License-Identifier: MIT

use crate::session::Attachment;
use flight_ui::SurfaceChoice;

/// A surface that is on screen, handed to whatever shows the workspace next without being let
/// go: its stream stays up, and what it sends from now on is read by the next holder. Dropping
/// it ends the stream.
pub struct Handover {
    pub choice: SurfaceChoice,
    pub attachment: Attachment,
    /// The size the attachment last heard of.
    pub size: (u16, u16),
    /// What the user typed after asking for the surfaces side by side, in the read that held the
    /// request: it is for the surface that has the keyboard next.
    pub typed_ahead: Vec<u8>,
}
