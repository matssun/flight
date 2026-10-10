// SPDX-License-Identifier: MIT

//! What every terminal engine must do, run against the engine Flight uses. A replacement is
//! tested by adding its type to `every_engine`.

use super::emulator::Emulator;
use super::engine::TerminalEngine;
use super::{Colour, Geometry, MouseMode};

fn every_engine(check: impl Fn(fn(u16, u16) -> Box<dyn Probe>)) {
    check(|c, r| Box::new(Emulator::new(geometry(c, r))));
}

fn geometry(cols: u16, rows: u16) -> Geometry {
    Geometry::new(cols, rows).expect("a size the test means to be valid")
}

/// `TerminalEngine` is not object safe (`new`), so the checks go through this.
trait Probe {
    fn feed(&mut self, bytes: &[u8]) -> bool;
    fn resize(&mut self, cols: u16, rows: u16);
    fn size(&self) -> (u16, u16);
    fn text(&self, col: u16, row: u16) -> Option<String>;
    fn fg(&self, col: u16, row: u16) -> Option<Colour>;
    fn cursor(&self) -> Option<(u16, u16)>;
    fn mouse(&self) -> MouseMode;
    fn alternate_screen(&self) -> bool;
}

impl<E: TerminalEngine> Probe for E {
    fn feed(&mut self, bytes: &[u8]) -> bool {
        TerminalEngine::feed(self, bytes).is_ok()
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        TerminalEngine::resize(self, geometry(cols, rows));
    }
    fn size(&self) -> (u16, u16) {
        TerminalEngine::size(self).pair()
    }
    fn text(&self, col: u16, row: u16) -> Option<String> {
        self.cell(col, row).map(|c| c.text.to_owned())
    }
    fn fg(&self, col: u16, row: u16) -> Option<Colour> {
        self.cell(col, row).map(|c| c.fg)
    }
    fn cursor(&self) -> Option<(u16, u16)> {
        TerminalEngine::cursor(self)
    }
    fn mouse(&self) -> MouseMode {
        self.modes().mouse
    }
    fn alternate_screen(&self) -> bool {
        self.modes().alternate_screen
    }
}

#[test]
fn a_new_engine_has_the_size_it_was_given_and_answers_every_cell_in_it() {
    every_engine(|new| {
        let e = new(30, 8);
        assert_eq!(e.size(), (30, 8));
        assert!(e.text(0, 0).is_some() && e.text(29, 7).is_some());
        assert!(e.text(30, 0).is_none() && e.text(0, 8).is_none());
    });
}

#[test]
fn what_is_written_is_drawn_with_its_colours_and_the_cursor_follows() {
    every_engine(|new| {
        let mut e = new(20, 4);
        assert!(e.feed(b"hi \x1b[31mred\x1b[0m"));
        assert_eq!(e.text(0, 0).as_deref(), Some("h"));
        assert_eq!(e.fg(3, 0), Some(Colour::Indexed(1)));
        assert_eq!(e.cursor(), Some((6, 0)));
        e.feed(b"\x1b[?25l");
        assert_eq!(e.cursor(), None);
    });
}

#[test]
fn modes_the_program_switched_on_are_reported() {
    every_engine(|new| {
        let mut e = new(20, 4);
        assert!(!e.alternate_screen() && e.mouse() == MouseMode::None);
        e.feed(b"\x1b[?1049h\x1b[?1002h");
        assert!(e.alternate_screen());
        assert_eq!(e.mouse(), MouseMode::Drag);
    });
}

#[test]
fn a_resize_is_reported_at_once_and_the_next_write_lands_at_the_new_size() {
    every_engine(|new| {
        let mut e = new(40, 10);
        e.feed(b"before");
        e.resize(20, 5);
        assert_eq!(e.size(), (20, 5));
        e.feed(b"\x1b[H\x1b[2Jafter");
        assert_eq!(e.text(0, 0).as_deref(), Some("a"));
        assert!(e.text(20, 0).is_none() && e.text(0, 5).is_none());
    });
}

#[test]
fn feeding_never_panics_whatever_the_size_or_the_bytes() {
    every_engine(|new| {
        let noise: Vec<u8> = (0..4000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let smallest = (Geometry::MIN_COLS, Geometry::MIN_ROWS);
        for (c, r) in [smallest, (2, 9), (9, 2), (80, 24)] {
            let mut e = new(c, r);
            e.feed(&noise);
            e.feed("日本\x1b[2J\x1b[5L\x1b[3;1H\u{1f642}".as_bytes());
            assert_eq!(e.size(), (c, r));
        }
    });
}

#[test]
fn rapid_resizes_with_output_between_always_end_at_the_last_size() {
    every_engine(|new| {
        let noise: Vec<u8> = (0..600u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
            .collect();
        let mut e = new(80, 24);
        let sizes = [
            (2, 2),
            (200, 60),
            (3, 2),
            (2, 40),
            (80, 24),
            (2, 2),
            (17, 5),
        ];
        for round in 0..700usize {
            let (c, r) = sizes[round % sizes.len()];
            e.resize(c, r);
            assert_eq!(e.size(), (c, r));
            if round % 3 != 0 {
                e.feed(&noise[round % 200..]);
            }
            assert_eq!(e.size().0, c);
        }
        e.resize(33, 7);
        e.feed(b"\x1b[H\x1b[2Jdone");
        assert_eq!(e.size(), (33, 7));
        assert_eq!(e.text(0, 0).as_deref(), Some("d"));
        assert!(e.text(33, 0).is_none() && e.text(0, 7).is_none());
    });
}

#[test]
fn a_resize_in_the_middle_of_an_escape_sequence_leaves_a_working_screen() {
    every_engine(|new| {
        let mut e = new(40, 10);
        e.feed(b"\x1b[3");
        e.resize(25, 6);
        e.feed(b"1mred\x1b[0m");
        assert_eq!(e.size(), (25, 6));
        e.feed(b"\x1b[H\x1b[2Jok");
        assert_eq!(e.text(0, 0).as_deref(), Some("o"));
    });
}
