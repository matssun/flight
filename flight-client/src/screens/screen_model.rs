// SPDX-License-Identifier: MIT

use super::cell::Colour;
use super::{CellView, Modes, MouseMode};

/// What one surface has drawn: one screen of cells. No scrollback is kept here (tmux, inside the
/// surface, keeps the history); the model costs about 150 KiB at 80x24 and 870 KiB at 200x60
/// with both the normal and the alternate screen full.
///
/// A resize does not resize the parser: `vt100` panics when a populated screen is resized (found
/// by feeding it random bytes with random resizes; feeding alone never panics, over 72 MB of
/// garbage). Instead the model starts a new blank screen of the new size and keeps showing the
/// old picture until the surface, which repaints everything when its size changes, writes the
/// first bytes of the new one.
pub struct ScreenModel {
    parser: vt100::Parser,
    resized: Option<vt100::Parser>,
}

fn convert(c: vt100::Color) -> Colour {
    match c {
        vt100::Color::Default => Colour::Default,
        vt100::Color::Idx(i) => Colour::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Colour::Rgb(r, g, b),
    }
}

/// The smallest screen the parser is given. `vt100` 0.15 panicked on any screen of one row (fed
/// random bytes, 200 of 200 runs at 80x1, 2x1 and 1x1) and 0.16.2 still panics at 1x1; it also
/// cannot place a double-width character in one column. From 2x2 up, 200 runs of 120 KB of
/// garbage each, at ten sizes, never panicked when only fed.
const MIN_COLS: u16 = 2;
const MIN_ROWS: u16 = 2;

/// Run `work`, and say whether it finished. What a program writes to a terminal is untrusted
/// input to a parser that is not ours; if it ever panics, the surface must lose its picture, not
/// take the dashboard (and every other surface) with it.
fn contain(work: impl FnOnce()) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_ok()
}

impl ScreenModel {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows.max(MIN_ROWS), cols.max(MIN_COLS), 0),
            resized: None,
        }
    }

    /// Take what the surface wrote. `false` means the parser failed on it: the model was
    /// emptied (same size) and the caller should have the surface redrawn.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        // The first bytes after a resize are the surface's repaint: they go to the new screen,
        // which then replaces the old picture.
        if let Some(mut fresh) = self.resized.take() {
            if contain(|| fresh.process(bytes)) {
                self.parser = fresh;
                return true;
            }
            self.parser = vt100::Parser::new(fresh.screen().size().0, fresh.screen().size().1, 0);
            return false;
        }
        let parser = &mut self.parser;
        if contain(|| parser.process(bytes)) {
            return true;
        }
        self.reset();
        false
    }

    /// The surface was told a new size. The old picture stays until the surface repaints.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (rows, cols) = (rows.max(MIN_ROWS), cols.max(MIN_COLS));
        if self.size() != (cols, rows) {
            self.resized = Some(vt100::Parser::new(rows, cols, 0));
        }
    }

    fn reset(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 0);
    }

    /// (columns, rows) the surface was last told, which the screen follows with its next write.
    pub fn size(&self) -> (u16, u16) {
        let (rows, cols) = self
            .resized
            .as_ref()
            .unwrap_or(&self.parser)
            .screen()
            .size();
        (cols, rows)
    }

    pub fn cell(&self, col: u16, row: u16) -> Option<CellView> {
        let cell = self.parser.screen().cell(row, col)?;
        Some(CellView {
            text: cell.contents().to_owned(),
            fg: convert(cell.fgcolor()),
            bg: convert(cell.bgcolor()),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
            wide: cell.is_wide(),
            continuation: cell.is_wide_continuation(),
        })
    }

    /// The row as plain text, trailing blanks removed.
    pub fn row_text(&self, row: u16) -> String {
        let (cols, _) = self.size();
        let mut text = String::new();
        for col in 0..cols {
            if let Some(cell) = self.parser.screen().cell(row, col) {
                if cell.is_wide_continuation() {
                    continue;
                }
                let t = cell.contents();
                text.push_str(if t.is_empty() { " " } else { t });
            }
        }
        text.trim_end().to_owned()
    }

    /// Where the cursor is, (column, row), unless the program hid it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
        if screen.hide_cursor() {
            return None;
        }
        let (row, col) = screen.cursor_position();
        Some((col, row))
    }

    pub fn modes(&self) -> Modes {
        let screen = self.parser.screen();
        Modes {
            alternate_screen: screen.alternate_screen(),
            bracketed_paste: screen.bracketed_paste(),
            application_cursor_keys: screen.application_cursor(),
            mouse: match screen.mouse_protocol_mode() {
                vt100::MouseProtocolMode::None => MouseMode::None,
                vt100::MouseProtocolMode::Press | vt100::MouseProtocolMode::PressRelease => {
                    MouseMode::Press
                }
                vt100::MouseProtocolMode::ButtonMotion => MouseMode::Drag,
                vt100::MouseProtocolMode::AnyMotion => MouseMode::Motion,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::contain;

    #[test]
    fn a_panic_in_the_parser_is_contained() {
        assert!(contain(|| ()));
        assert!(!contain(|| panic!("the parser failed")));
    }
}
