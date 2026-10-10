// SPDX-License-Identifier: MIT

use super::{CellView, Modes};

/// What Flight needs from a terminal emulator, and nothing more: take the bytes a surface wrote,
/// keep the screen they draw, and say what is on it. The terminal emulation library is behind
/// this; nothing outside `screens` names it, and a replacement is one more implementation of
/// this trait plus the one line in `screen_model.rs` that picks it.
///
/// The emulator is a presentation component. It never decides anything about a session, a
/// workspace or a tmux server, and everything in it can be rebuilt from the live surface (which
/// repaints itself whenever its size changes).
///
/// Invariants every implementation keeps (`engine_contract_tests` runs them against each):
///
/// - `feed` never panics and never ends the process. If the emulator fails on some input it
///   empties the screen (same size) and returns `false`, so the caller can have the surface
///   redrawn.
/// - `size` is the (columns, rows) most recently asked for by `new` or `resize`, and `cell`
///   answers every position inside it. A position outside it is `None`.
/// - After `resize`, the old picture may stay until the surface writes its first bytes at the
///   new size.
pub(super) trait TerminalEngine {
    fn new(cols: u16, rows: u16) -> Self;

    /// Take what the surface wrote; `false` means the emulator failed on it (see above).
    fn feed(&mut self, bytes: &[u8]) -> bool;

    /// The surface was told a new size.
    fn resize(&mut self, cols: u16, rows: u16);

    /// (columns, rows).
    fn size(&self) -> (u16, u16);

    /// The cell at (column, row), if that is on the screen.
    fn cell(&self, col: u16, row: u16) -> Option<CellView<'_>>;

    /// Where the cursor is, (column, row), unless the program hid it.
    fn cursor(&self) -> Option<(u16, u16)>;

    /// The terminal modes the program has switched on.
    fn modes(&self) -> Modes;
}
