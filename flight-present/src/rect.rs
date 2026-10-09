// SPDX-License-Identifier: MIT

/// A rectangle of terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, cols: u16, rows: u16) -> Self {
        Self { x, y, cols, rows }
    }

    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.cols)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.rows)
    }

    pub fn cells(&self) -> u32 {
        u32::from(self.cols).saturating_mul(u32::from(self.rows))
    }

    pub fn is_empty(&self) -> bool {
        self.cols == 0 || self.rows == 0
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// How many columns the two share when seen from the side (the length of the overlap of
    /// their column ranges).
    pub fn shared_cols(&self, other: &Rect) -> u16 {
        overlap(self.x, self.right(), other.x, other.right())
    }

    /// How many rows the two share.
    pub fn shared_rows(&self, other: &Rect) -> u16 {
        overlap(self.y, self.bottom(), other.y, other.bottom())
    }

    pub fn center(&self) -> (u32, u32) {
        (
            u32::from(self.x)
                .saturating_mul(2)
                .saturating_add(u32::from(self.cols)),
            u32::from(self.y)
                .saturating_mul(2)
                .saturating_add(u32::from(self.rows)),
        )
    }
}

fn overlap(a0: u16, a1: u16, b0: u16, b1: u16) -> u16 {
    a1.min(b1).saturating_sub(a0.max(b0))
}
