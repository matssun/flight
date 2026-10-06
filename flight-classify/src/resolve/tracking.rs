// SPDX-License-Identifier: MIT

/// Per-pane memory carried between resolutions: what a one-shot classification cannot know.
/// In Fleet this lives in process memory and a cold start simply never synthesizes Done;
/// the owner of a `Tracking` decides how long it survives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tracking {
    /// The pane was last seen working (or blocked on a prompt), so a drop to idle is a
    /// finished turn.
    pub was_busy: bool,
    /// The turn finished while the user was elsewhere and has not been seen or restarted.
    pub done: bool,
    /// When the working glyph was last seen (seconds since the epoch).
    pub glyph_anchor: Option<u64>,
}
