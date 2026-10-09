// SPDX-License-Identifier: MIT

use crate::Rect;
use flight_state::SurfaceId;

/// Where a showing surface goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub surface: SurfaceId,
    pub area: Rect,
    pub focused: bool,
}

/// A line between two siblings of a split, one cell thick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divider {
    pub area: Rect,
    /// A vertical line (between side-by-side children) rather than a horizontal one.
    pub vertical: bool,
}

/// The one-row strip above a set of tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabBar {
    pub area: Rect,
    /// The first surface of each tab, to be named by whoever draws the bar.
    pub tabs: Vec<SurfaceId>,
    pub active: usize,
}
