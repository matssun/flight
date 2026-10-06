// SPDX-License-Identifier: MIT

use super::ResolvedState;
use crate::Evidence;

/// Fleet's default grace period before a vanished glyph reads as idle.
pub const DEFAULT_IDLE_SECS: u64 = 3;

/// Everything one resolution needs: what happened before, what is seen now, and the time.
#[derive(Debug, Clone)]
pub struct ResolveInput<'a> {
    /// The previous resolution for this pane; `None` on first sight (a cold start never
    /// synthesizes Done).
    pub previous: Option<&'a ResolvedState>,
    /// This tick's evidence. `evidence.working_glyph` is ignored: the resolver derives it
    /// from `glyph_seen` and the previous glyph anchor.
    pub evidence: &'a Evidence,
    /// The raw working glyph is on screen right now (before debouncing).
    pub glyph_seen: bool,
    /// Seconds a vanished glyph still counts as working.
    pub idle_secs: u64,
}
