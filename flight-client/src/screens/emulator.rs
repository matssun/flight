// SPDX-License-Identifier: MIT

//! The terminal emulator: `vt100`, and every assumption about how it behaves. This is the only
//! file that names it.

use super::engine::TerminalEngine;
use super::insert_guard::InsertGuard;
use super::EngineFailure;
use super::{CellView, Colour, Geometry, Modes, MouseEncoding, MouseMode};
use std::cell::{Cell, RefCell};
use std::sync::Once;

/// A resize does not resize the parser: `vt100` panics when a populated screen is resized (found
/// by feeding it random bytes with random resizes; feeding alone never panics, over 72 MB of
/// garbage). Instead this starts a new blank screen of the new size and keeps showing the old
/// picture until the surface, which repaints everything when its size changes, writes the first
/// bytes of the new one.
pub(super) struct Emulator {
    parser: vt100::Parser,
    resized: Option<vt100::Parser>,
    guard: InsertGuard,
}

fn convert(c: vt100::Color) -> Colour {
    match c {
        vt100::Color::Default => Colour::Default,
        vt100::Color::Idx(i) => Colour::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Colour::Rgb(r, g, b),
    }
}

thread_local! {
    /// Set while a parser call is being contained on this thread, so the panic hook stays quiet
    /// (a message on stderr would be drawn into the terminal Flight is running in) and keeps
    /// what the panic said.
    static CONTAINING: Cell<bool> = const { Cell::new(false) };
    static SAID: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Make panics inside [`contain`] silent and recorded, and leave every other panic to the hook
/// that was there.
fn install_quiet_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !CONTAINING.with(Cell::get) {
                before(info);
                return;
            }
            let payload = info.payload();
            let what = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic without a message".to_owned());
            let said = match info.location() {
                Some(at) => format!("{what} (at {}:{})", at.file(), at.line()),
                None => what,
            };
            SAID.with(|slot| *slot.borrow_mut() = Some(said));
        }));
    });
}

/// Run `work`, and say what the parser reported if it panicked. What a program writes to a
/// terminal is untrusted input to a parser that is not ours; if it ever panics, the surface must
/// lose its picture, not take the dashboard (and every other surface) with it.
fn contain(work: impl FnOnce()) -> Result<(), String> {
    install_quiet_hook();
    CONTAINING.with(|flag| flag.set(true));
    let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    CONTAINING.with(|flag| flag.set(false));
    finished.map_err(|_| {
        SAID.with(|slot| slot.borrow_mut().take())
            .unwrap_or_else(|| "a panic without a message".to_owned())
    })
}

impl Emulator {
    fn failure(&self, message: String, bytes: usize) -> EngineFailure {
        EngineFailure {
            message,
            bytes,
            geometry: self.size(),
        }
    }

    fn reset(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 0);
        self.guard = InsertGuard::default();
    }
}

impl TerminalEngine for Emulator {
    fn new(geometry: Geometry) -> Self {
        Self {
            parser: vt100::Parser::new(geometry.rows(), geometry.cols(), 0),
            resized: None,
            guard: InsertGuard::default(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), EngineFailure> {
        let given = bytes.len();
        let guarded = self.guard.pass(bytes, self.size().cols());
        let bytes = guarded.as_slice();
        // The first bytes after a resize are the surface's repaint: they go to the new screen,
        // which then replaces the old picture.
        if let Some(mut fresh) = self.resized.take() {
            return match contain(|| fresh.process(bytes)) {
                Ok(()) => {
                    self.parser = fresh;
                    Ok(())
                }
                Err(message) => {
                    let (rows, cols) = fresh.screen().size();
                    self.parser = vt100::Parser::new(rows, cols, 0);
                    self.guard = InsertGuard::default();
                    Err(self.failure(message, given))
                }
            };
        }
        let parser = &mut self.parser;
        match contain(|| parser.process(bytes)) {
            Ok(()) => Ok(()),
            Err(message) => {
                self.reset();
                Err(self.failure(message, given))
            }
        }
    }

    fn resize(&mut self, geometry: Geometry) {
        if self.size() != geometry {
            self.resized = Some(vt100::Parser::new(geometry.rows(), geometry.cols(), 0));
            self.guard = InsertGuard::default();
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
                vt100::MouseProtocolMode::Press => MouseMode::Press,
                vt100::MouseProtocolMode::PressRelease => MouseMode::PressRelease,
                vt100::MouseProtocolMode::ButtonMotion => MouseMode::Drag,
                vt100::MouseProtocolMode::AnyMotion => MouseMode::Motion,
            },
            mouse_encoding: match screen.mouse_protocol_encoding() {
                vt100::MouseProtocolEncoding::Default => MouseEncoding::Default,
                vt100::MouseProtocolEncoding::Utf8 => MouseEncoding::Utf8,
                vt100::MouseProtocolEncoding::Sgr => MouseEncoding::Sgr,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{contain, Emulator};
    use crate::screens::engine::TerminalEngine;
    use crate::screens::Geometry;

    #[test]
    fn a_panic_in_the_parser_is_contained_and_says_what_it_was_and_where() {
        assert_eq!(contain(|| ()), Ok(()));
        let said = contain(|| panic!("the parser failed")).unwrap_err();
        assert!(said.starts_with("the parser failed (at "), "{said}");
        assert!(said.contains("emulator.rs"), "{said}");
        // The next one is reported on its own.
        let again = contain(|| panic!("{}", String::from("another"))).unwrap_err();
        assert!(again.starts_with("another"), "{again}");
    }

    /// What the library does with the bytes as they are, to compare with what Flight feeds it.
    fn raw(cols: u16, rows: u16, bytes: &[u8]) -> Vec<u8> {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes);
        parser.screen().contents_formatted()
    }

    fn guarded(cols: u16, rows: u16, bytes: &[u8]) -> Vec<u8> {
        let mut engine = Emulator::new(Geometry::new(cols, rows).unwrap());
        engine.feed(bytes).unwrap();
        engine.parser.screen().contents_formatted()
    }

    #[test]
    fn writing_the_width_as_the_count_leaves_the_screen_as_the_larger_count_does() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let texts = [
            "hello world",
            "日本語のテキスト",
            "a日b本c",
            "e\u{301}x\u{301}y",
            "👨\u{200d}👩 ok",
        ];
        for _ in 0..300 {
            let (cols, rows) = (2 + next(30) as u16, 2 + next(6) as u16);
            let mut setup = Vec::new();
            for _ in 0..rows {
                setup.extend_from_slice(texts[next(5) as usize].as_bytes());
                setup.extend_from_slice(b"\x1b[1m\r\n\x1b[0m");
            }
            setup.extend_from_slice(
                format!(
                    "\x1b[{};{}H",
                    1 + next(u64::from(rows)),
                    1 + next(u64::from(cols))
                )
                .as_bytes(),
            );
            let big = 1 + u64::from(cols) + next(1500);
            let with = |count: u64| [setup.clone(), format!("\x1b[{count}@").into_bytes()].concat();
            assert_eq!(
                guarded(cols, rows, &with(big)),
                raw(cols, rows, &with(big)),
                "{cols}x{rows} count {big}"
            );
        }
    }

    #[test]
    fn a_count_that_would_take_the_library_seconds_takes_no_time_in_any_form_it_can_be_written() {
        let mut engine = Emulator::new(Geometry::new(80, 24).unwrap());
        engine.feed(b"hello\x1b[1;3H").unwrap();
        let started = std::time::Instant::now();
        for form in [
            &b"\x1b[65535@"[..],
            b"\x1b[4294967295@",
            b"\x1b[99999999999999999999@",
            b"\x1b[0000000065535@",
            b"\x1b[65535;1@",
            b"\x1b[65535:1@",
            b"\x1b[6\n5535@",
        ] {
            engine.feed(form).unwrap();
        }
        // Cut in pieces, as a stream is.
        for piece in [&b"\x1b[6"[..], b"55", b"35@"] {
            engine.feed(piece).unwrap();
        }
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "took {:?}",
            started.elapsed()
        );
        assert_eq!(
            engine.cell(0, 0).map(|c| c.text.to_owned()),
            Some("h".to_owned())
        );
    }

    /// Escape-sequence soup at every smallest size a `Geometry` allows and a few larger: the
    /// parser must neither panic nor take long (a seeded stream, so a failure repeats). Found
    /// the count-of-`@` stall; found no panic in 3.2 million streams of this kind (ADR-011).
    #[test]
    fn structured_garbage_at_valid_sizes_neither_panics_nor_stalls() {
        const PARAMS: &[&str] = &[
            "",
            "0",
            "1",
            "2",
            "5",
            "24",
            "80",
            "255",
            "1000",
            "65535",
            "65536",
            "4294967295",
            ";",
            "1;1",
            "0;0",
            "999;1",
            "?",
            "?1049",
            "?25",
            "?6",
            "?69",
            "?47",
            "38;5;300",
            "38;2;1;2;3",
            "48;2;999;0;0",
            ":",
            "1:2",
        ];
        const FINALS: &str = "@ABCDEFGHIJKLMPSTXZ`abcdefghilmnpqrstuvwxyz{|}~";
        const TEXT: &[&str] = &[
            "a",
            "bc",
            "日本",
            "e\u{301}",
            "\u{200d}",
            "👨\u{200d}👩",
            "\u{0}",
            "\t",
            "\r",
            "\n",
            "\u{8}",
            "\u{b}",
            "\u{c}",
            "\u{e}",
            "\u{f}",
            "\u{ffff}",
            "ｱ",
            "０",
            "\u{fe0f}",
        ];
        const ESC: &[&str] = &[
            "c", "7", "8", "D", "E", "M", "H", "=", ">", "(0", "(B", "#8", "N",
        ];
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % n as u64).unwrap_or(0)
        };
        let started = std::time::Instant::now();
        for _ in 0..5_000 {
            let (cols, rows) = ([2u16, 3, 5, 80][next(4)], [2u16, 3, 4, 24][next(4)]);
            let mut bytes = Vec::new();
            for _ in 0..1 + next(40) {
                match next(10) {
                    0..=3 => bytes.extend_from_slice(TEXT[next(TEXT.len())].as_bytes()),
                    4..=7 => {
                        bytes.extend_from_slice(b"\x1b[");
                        for _ in 0..next(3) {
                            bytes.extend_from_slice(PARAMS[next(PARAMS.len())].as_bytes());
                        }
                        bytes.push(FINALS.as_bytes()[next(FINALS.len())]);
                    }
                    8 => {
                        bytes.push(0x1b);
                        bytes.extend_from_slice(ESC[next(ESC.len())].as_bytes());
                    }
                    _ => bytes.push(u8::try_from(next(256)).unwrap_or(0)),
                }
            }
            let mut engine = Emulator::new(Geometry::new(cols, rows).unwrap());
            for piece in bytes.chunks(1 + next(9)) {
                engine
                    .feed(piece)
                    .unwrap_or_else(|f| panic!("{cols}x{rows} {bytes:?}: {f}"));
            }
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
    }
}
