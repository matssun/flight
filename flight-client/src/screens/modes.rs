// SPDX-License-Identifier: MIT

/// How a program wants mouse input reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMode {
    None,
    /// Presses and releases only.
    Press,
    /// Plus drags.
    Drag,
    /// Plus all movement.
    Motion,
}

/// The terminal modes the program on a screen has switched on. The real terminal has to be in
/// the matching mode while that screen has the keyboard, or the program is sent input it did
/// not ask for (or none it did).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    pub alternate_screen: bool,
    pub bracketed_paste: bool,
    pub application_cursor_keys: bool,
    pub mouse: MouseMode,
}
