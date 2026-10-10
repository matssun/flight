// SPDX-License-Identifier: MIT

use super::emulator::Emulator;
use super::engine::TerminalEngine;
use super::{CellView, Geometry, Modes};

/// The one place that says which terminal emulator Flight uses.
type Engine = Emulator;

/// What one surface has drawn: one screen of cells. No scrollback is kept here (tmux, inside the
/// surface, keeps the history); the model costs about 150 KiB at 80x24 and 870 KiB at 200x60
/// with both the normal and the alternate screen full.
///
/// Everything that depends on how the emulator behaves is behind [`TerminalEngine`]; this type
/// is what the rest of Flight holds.
pub struct ScreenModel {
    engine: Engine,
}

impl ScreenModel {
    pub fn new(geometry: Geometry) -> Self {
        Self {
            engine: Engine::new(geometry),
        }
    }

    /// Take what the surface wrote. `false` means the emulator failed on it: the model was
    /// emptied (same size) and the caller should have the surface redrawn.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        self.engine.feed(bytes)
    }

    /// The surface was told a new size. The old picture stays until the surface repaints.
    pub fn resize(&mut self, geometry: Geometry) {
        self.engine.resize(geometry);
    }

    /// The geometry the surface was last told, which the screen follows with its next write.
    pub fn geometry(&self) -> Geometry {
        self.engine.size()
    }

    /// (columns, rows) of [`Self::geometry`].
    pub fn size(&self) -> (u16, u16) {
        self.engine.size().pair()
    }

    pub fn cell(&self, col: u16, row: u16) -> Option<CellView<'_>> {
        self.engine.cell(col, row)
    }

    /// The row as plain text, trailing blanks removed.
    pub fn row_text(&self, row: u16) -> String {
        let (cols, _) = self.size();
        let mut text = String::new();
        for col in 0..cols {
            if let Some(cell) = self.engine.cell(col, row) {
                if cell.continuation {
                    continue;
                }
                text.push_str(if cell.text.is_empty() { " " } else { cell.text });
            }
        }
        text.trim_end().to_owned()
    }

    /// Where the cursor is, (column, row), unless the program hid it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.engine.cursor()
    }

    pub fn modes(&self) -> Modes {
        self.engine.modes()
    }
}
