// SPDX-License-Identifier: MIT

use super::Geometry;
use std::fmt;

/// The terminal emulator failed on what a surface wrote. The screen was emptied (at the same
/// geometry) and is usable again; what it showed is gone and has to be drawn again by the
/// surface. It says what happened, so the failure can be reported and traced instead of only
/// noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineFailure {
    /// What the emulator reported, with where, when it said.
    pub message: String,
    /// How many bytes it was given when it failed.
    pub bytes: usize,
    /// The geometry of the screen that was emptied.
    pub geometry: Geometry,
}

impl fmt::Display for EngineFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the terminal emulator failed on {} bytes at {}x{}: {}",
            self.bytes,
            self.geometry.cols(),
            self.geometry.rows(),
            self.message
        )
    }
}

impl std::error::Error for EngineFailure {}
