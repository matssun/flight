// SPDX-License-Identifier: MIT

/// How a program wants mouse input reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MouseMode {
    /// Nothing is reported. Each mode below is told everything the one before it is.
    None,
    /// Button presses only (X10, `?9`).
    Press,
    /// Presses and releases (`?1000`).
    PressRelease,
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
    /// How the program wants a mouse report written (`?1005`, `?1006`).
    pub mouse_encoding: MouseEncoding,
}

/// How a mouse report is written to the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEncoding {
    /// `ESC [ M` and three bytes; positions past 222 cannot be written.
    Default,
    /// The same, with positions as UTF-8 characters (up to 2014).
    Utf8,
    /// `ESC [ < b ; x ; y` and `M` or `m`: no limit on position, and releases say which button.
    Sgr,
}
