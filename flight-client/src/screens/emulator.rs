// SPDX-License-Identifier: MIT

//! The terminal emulator: `vt100`, and every assumption about how it behaves. This is the only
//! file that names it.

use super::engine::TerminalEngine;
use super::{CellView, Colour, Geometry, Modes, MouseMode};

/// A resize does not resize the parser: `vt100` panics when a populated screen is resized (found
/// by feeding it random bytes with random resizes; feeding alone never panics, over 72 MB of
/// garbage). Instead this starts a new blank screen of the new size and keeps showing the old
/// picture until the surface, which repaints everything when its size changes, writes the first
/// bytes of the new one.
pub(super) struct Emulator {
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

/// Run `work`, and say whether it finished. What a program writes to a terminal is untrusted
/// input to a parser that is not ours; if it ever panics, the surface must lose its picture, not
/// take the dashboard (and every other surface) with it.
fn contain(work: impl FnOnce()) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_ok()
}

impl Emulator {
    fn reset(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 0);
    }
}

impl TerminalEngine for Emulator {
    fn new(geometry: Geometry) -> Self {
        Self {
            parser: vt100::Parser::new(geometry.rows(), geometry.cols(), 0),
            resized: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> bool {
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

    fn resize(&mut self, geometry: Geometry) {
        if self.size() != geometry {
            self.resized = Some(vt100::Parser::new(geometry.rows(), geometry.cols(), 0));
        }
    }

    fn size(&self) -> Geometry {
        let (rows, cols) = self
            .resized
            .as_ref()
            .unwrap_or(&self.parser)
            .screen()
            .size();
        // Both parsers were made from a `Geometry`, so this always holds.
        Geometry::new(cols, rows).unwrap_or(Geometry::STANDARD)
    }

    fn cell(&self, col: u16, row: u16) -> Option<CellView<'_>> {
        let cell = self.parser.screen().cell(row, col)?;
        Some(CellView {
            text: cell.contents(),
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

    fn cursor(&self) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
        if screen.hide_cursor() {
            return None;
        }
        let (row, col) = screen.cursor_position();
        Some((col, row))
    }

    fn modes(&self) -> Modes {
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
