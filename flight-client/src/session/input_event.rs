// SPDX-License-Identifier: MIT

use flight_ui::SurfaceChoice;

/// One thing the user did at the keyboard, in the order they did it. A session never reorders
/// these: bytes typed after a surface switch belong to the new surface even if it is not up yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// Bytes for the surface that is current at this point of the input.
    Data(Vec<u8>),
    /// `Ctrl-Space a` or `s`: the following input is for this surface.
    Switch(SurfaceChoice),
    /// `Ctrl-Space q`: leave the session.
    Leave,
    /// `Ctrl-Space v`: show this workspace's surfaces side by side. Takes effect once the
    /// bytes typed before it have been delivered.
    Present,
    /// `Ctrl-Space` and an unknown key: remind the user of the keys.
    Hint,
}
