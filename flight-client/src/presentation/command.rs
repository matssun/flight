// SPDX-License-Identifier: MIT

use flight_present::{Axis, Direction};

/// What the user can do to the layout while presenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Show another surface next to the focused one (side by side for `Across`).
    Split(Axis),
    /// Show another surface as a tab where the focused one is.
    NewTab,
    /// Stop showing the focused surface. The surface itself keeps running.
    Close,
    /// The next or previous tab of the focused surface's tab set.
    StepTab(bool),
    Focus(Direction),
    /// The next showing surface in reading order.
    FocusNext,
    /// Make the focused surface bigger (`true`) or smaller along an axis.
    Grow(Axis, bool),
    /// Show the workspace's agent or shell where the focused surface is.
    ShowHere(Shown),
}

/// The surfaces every workspace has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Agent,
    Shell,
}
