// SPDX-License-Identifier: MIT

/// Which way a split divides its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Children side by side, left to right, divided by vertical lines.
    Across,
    /// Children stacked, top to bottom, divided by horizontal lines.
    Down,
}
