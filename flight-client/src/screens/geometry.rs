// SPDX-License-Identifier: MIT

use std::fmt;

/// The size of a screen: columns and rows, each at least [`Geometry::MIN_COLS`] and
/// [`Geometry::MIN_ROWS`]. A smaller size cannot be made, so nothing holds one.
///
/// This is the *effective* size: what the emulator and the surface's pseudo-terminal both use,
/// always the same. The *viewport* is the area a layout gives a tile, which can be anything,
/// zero included. When the viewport is too small for a geometry, the screen and the surface keep
/// their last geometry and the tile shows a placeholder; nothing is clamped to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    cols: u16,
    rows: u16,
}

/// A size no screen can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooSmall {
    pub cols: u16,
    pub rows: u16,
}

impl fmt::Display for TooSmall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} is below the smallest screen, {}x{}",
            self.cols,
            self.rows,
            Geometry::MIN_COLS,
            Geometry::MIN_ROWS
        )
    }
}

impl std::error::Error for TooSmall {}

impl Geometry {
    /// The emulator library panics on one-row screens and on a 1x1 screen, and cannot place a
    /// double-width character in one column (see `emulator.rs`).
    pub const MIN_COLS: u16 = 2;
    pub const MIN_ROWS: u16 = 2;

    /// What a screen is made at when there is not yet a viewport it can follow.
    pub const STANDARD: Geometry = Geometry { cols: 80, rows: 24 };

    pub fn new(cols: u16, rows: u16) -> Result<Self, TooSmall> {
        if cols < Self::MIN_COLS || rows < Self::MIN_ROWS {
            return Err(TooSmall { cols, rows });
        }
        Ok(Self { cols, rows })
    }

    pub fn cols(self) -> u16 {
        self.cols
    }

    pub fn rows(self) -> u16 {
        self.rows
    }

    pub fn pair(self) -> (u16, u16) {
        (self.cols, self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_below_the_smallest_screen_can_be_made() {
        for (c, r) in [(0, 0), (1, 1), (0, 24), (80, 0), (1, 24), (80, 1), (1, 80)] {
            assert_eq!(Geometry::new(c, r), Err(TooSmall { cols: c, rows: r }));
        }
        let smallest = Geometry::new(2, 2).unwrap();
        assert_eq!(smallest.pair(), (2, 2));
        assert_eq!(Geometry::new(u16::MAX, u16::MAX).unwrap().cols(), u16::MAX);
    }

    #[test]
    fn the_refusal_says_what_was_asked_and_what_is_least() {
        let said = Geometry::new(1, 0).unwrap_err().to_string();
        assert_eq!(said, "1x0 is below the smallest screen, 2x2");
    }
}
