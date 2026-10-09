// SPDX-License-Identifier: MIT

//! Composable presentation (ADR-011): how several surfaces share one terminal, kept apart from
//! what a surface is and how long it lives.
//!
//! A [`Layout`] is a tree of [`Region`]s whose leaves name surfaces by their `SurfaceId`: a
//! single surface, side by side, stacked, nested, in tabs. Nothing here opens, ends or even
//! knows about a surface beyond its identity; removing a leaf hides a surface, it does not end
//! it. The module is pure (no I/O, no clock): [`solve`] turns a layout and a terminal size into
//! rectangles, the `focus` functions move the cursor between them, and [`saved`] is the form a
//! layout takes on disk.

mod axis;
mod child;
mod edit;
mod focus;
mod layout;
mod layout_error;
mod rect;
mod region;
pub mod saved;
mod solve;
mod style;
mod tile;

pub use axis::Axis;
pub use child::Child;
pub use edit::Placement;
pub use focus::{cycle, neighbor, Direction};
pub use layout::{Layout, MAX_DEPTH, MAX_SURFACES, MAX_WEIGHT};
pub use layout_error::LayoutError;
pub use rect::Rect;
pub use region::Region;
pub use solve::{solve, Solved};
pub use style::Style;
pub use tile::{Divider, TabBar, Tile};
