// SPDX-License-Identifier: MIT

use super::cell::Colour;
use super::{paint, MouseMode, ScreenModel, Theme};
use flight_present::{solve, Axis, Child, Layout, Rect, Region, Style as LayoutStyle};
use flight_state::SurfaceId;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

fn id(s: &str) -> SurfaceId {
    SurfaceId::new(s)
}

#[test]
fn text_attributes_and_colours_are_kept() {
    let mut m = ScreenModel::new(40, 5);
    m.feed(b"plain \x1b[1;31mbold red\x1b[0m \x1b[4mu\x1b[0m\x1b[7mi\x1b[0m\r\n\x1b[48;5;21mblue\x1b[0m \x1b[38;2;1;2;3mrgb\x1b[0m");
    assert_eq!(m.row_text(0), "plain bold red ui");
    let red = m.cell(6, 0).unwrap();
    assert!(red.bold && red.fg == Colour::Indexed(1), "{red:?}");
    assert!(m.cell(15, 0).unwrap().underline);
    assert!(m.cell(16, 0).unwrap().inverse);
    assert_eq!(m.cell(0, 1).unwrap().bg, Colour::Indexed(21));
    assert_eq!(m.cell(5, 1).unwrap().fg, Colour::Rgb(1, 2, 3));
}

#[test]
fn wide_and_combining_characters_take_the_cells_they_should() {
    let mut m = ScreenModel::new(20, 2);
    m.feed("日本e\u{301}🙂x".as_bytes());
    assert_eq!(m.row_text(0), "日本e\u{301}🙂x");
    let first = m.cell(0, 0).unwrap();
    assert!(first.wide && !first.continuation);
    assert!(m.cell(1, 0).unwrap().continuation);
    // 日本 take four cells, e+accent one, the emoji two, then x.
    assert_eq!(m.cell(4, 0).unwrap().text, "e\u{301}");
    assert_eq!(m.cell(7, 0).unwrap().text, "x");
}

#[test]
fn the_cursor_and_its_hiding_are_reported() {
    let mut m = ScreenModel::new(20, 5);
    m.feed(b"ab\r\ncd");
    assert_eq!(m.cursor(), Some((2, 1)));
    m.feed(b"\x1b[?25l");
    assert_eq!(m.cursor(), None);
    m.feed(b"\x1b[?25h\x1b[3;4H");
    assert_eq!(m.cursor(), Some((3, 2)));
}

#[test]
fn the_modes_the_program_asks_for_are_visible() {
    let mut m = ScreenModel::new(20, 5);
    let none = m.modes();
    assert!(!none.alternate_screen && !none.bracketed_paste && !none.application_cursor_keys);
    assert_eq!(none.mouse, MouseMode::None);
    m.feed(b"\x1b[?1049h\x1b[?2004h\x1b[?1h\x1b[?1002h");
    let on = m.modes();
    assert!(on.alternate_screen && on.bracketed_paste && on.application_cursor_keys);
    assert_eq!(on.mouse, MouseMode::Drag);
    m.feed(b"\x1b[?1049l\x1b[?2004l\x1b[?1l\x1b[?1002l\x1b[?1000h");
    let off = m.modes();
    assert!(!off.alternate_screen && !off.bracketed_paste && !off.application_cursor_keys);
    assert_eq!(off.mouse, MouseMode::Press);
}

#[test]
fn a_sequence_cut_between_two_feeds_is_not_lost() {
    let mut m = ScreenModel::new(20, 2);
    m.feed(b"\x1b[3");
    m.feed(b"1mred\x1b[0m");
    assert_eq!(m.cell(0, 0).unwrap().fg, Colour::Indexed(1));
    assert_eq!(m.row_text(0), "red");
}

#[test]
fn a_resize_keeps_the_old_picture_until_the_surface_repaints_at_the_new_size() {
    let mut m = ScreenModel::new(40, 10);
    m.feed(b"hello");
    m.resize(20, 5);
    assert_eq!(m.size(), (20, 5));
    assert_eq!(
        m.row_text(0),
        "hello",
        "no blank flash while the surface redraws"
    );
    // The first bytes of the repaint are the new picture.
    assert!(m.feed(b"\x1b[H\x1b[2Jworld"));
    assert_eq!(m.row_text(0), "world");
    assert_eq!(m.size(), (20, 5));
    // Asking for the size it already has changes nothing.
    m.resize(20, 5);
    m.feed(b"!");
    assert_eq!(m.row_text(0), "world!");
    m.resize(0, 0);
    assert_eq!(m.size(), (2, 2), "never smaller than the parser can use");
}

#[test]
fn whatever_a_program_writes_cannot_break_the_model() {
    // Bytes of every kind, including broken escape sequences and invalid UTF-8.
    let mut x: u64 = 0x1234_5678_9abc_def0;
    let mut m = ScreenModel::new(80, 24);
    for _ in 0..200 {
        let mut chunk = Vec::new();
        for _ in 0..4096 {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let b = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
            // A lot of escapes, so the parser spends its time in the interesting places.
            chunk.push(if b < 24 { 0x1b } else { b });
        }
        assert!(m.feed(&chunk));
        m.resize(1 + (chunk[0] as u16 % 120), 1 + (chunk[1] as u16 % 50));
    }
    let (cols, rows) = m.size();
    assert!(cols >= 2 && rows >= 2);
    let _ = m.modes();
    let _ = m.cursor();
}

fn buffer(cols: u16, rows: u16) -> Buffer {
    Buffer::empty(ratatui::layout::Rect::new(0, 0, cols, rows))
}

fn symbols(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_owned())
        .collect::<Vec<_>>()
        .concat()
}

fn two_up() -> Layout {
    Layout::new(
        Region::Split {
            axis: Axis::Across,
            children: vec![
                Child::new(1, Region::Surface(id("agent"))),
                Child::new(1, Region::Surface(id("shell"))),
            ],
        },
        id("agent"),
    )
    .unwrap()
}

#[test]
fn two_screens_are_drawn_side_by_side_with_a_line_and_the_cursor_of_the_focused_one() {
    let layout = two_up();
    let solved = solve(
        &layout,
        Rect::new(0, 0, 41, 6),
        &LayoutStyle {
            min_cols: 10,
            min_rows: 3,
        },
    );
    let (a, s) = (
        solved.tile(&id("agent")).unwrap().area,
        solved.tile(&id("shell")).unwrap().area,
    );
    let mut agent = ScreenModel::new(a.cols, a.rows);
    let mut shell = ScreenModel::new(s.cols, s.rows);
    agent.feed(b"\x1b[1;32magent here\x1b[0m\r\n> typing");
    shell.feed(b"$ ls\r\nfile.txt");
    let mut buf = buffer(41, 6);
    let painted = paint(
        &mut buf,
        &solved,
        &|surface| match surface.as_str() {
            "agent" => Some(&agent),
            "shell" => Some(&shell),
            _ => None,
        },
        &|surface| surface.to_string(),
        &Theme::default(),
    );
    let row0 = symbols(&buf, 0);
    assert!(row0.starts_with("agent here"), "{row0:?}");
    assert!(row0.contains("│$ ls"), "{row0:?}");
    assert_eq!(&symbols(&buf, 1)[..8], "> typing");
    assert!(symbols(&buf, 1).contains("│file.txt"));
    // Attributes survive into the buffer.
    assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    assert_eq!(buf[(0, 0)].fg, Color::Indexed(2));
    // The line next to the focused tile is marked; the cursor is the focused screen's.
    assert_eq!(buf[(a.cols, 0)].symbol(), "│");
    assert_eq!(buf[(a.cols, 0)].fg, Color::Cyan);
    assert_eq!(painted.cursor, Some((8, 1)));
}

#[test]
fn a_wide_character_never_spills_into_the_next_tile_and_a_blank_tile_is_blank() {
    let layout = two_up();
    let solved = solve(
        &layout,
        Rect::new(0, 0, 41, 6),
        &LayoutStyle {
            min_cols: 10,
            min_rows: 3,
        },
    );
    let a = solved.tile(&id("agent")).unwrap().area;
    // A model wider than its tile (the surface has not heard of the new size yet).
    let mut agent = ScreenModel::new(a.cols + 4, a.rows);
    agent.feed("x".repeat(usize::from(a.cols) - 1).as_bytes());
    agent.feed("日本".as_bytes());
    let mut buf = buffer(41, 6);
    paint(
        &mut buf,
        &solved,
        &|surface| (surface.as_str() == "agent").then_some(&agent),
        &|surface| surface.to_string(),
        &Theme::default(),
    );
    assert_eq!(buf[(a.cols, 0)].symbol(), "│", "the line is intact");
    assert_eq!(
        buf[(a.cols - 1, 0)].symbol(),
        " ",
        "the wide character that does not fit is not half drawn"
    );
    // The shell has no model yet: its tile is empty, not garbage.
    let after: String = symbols(&buf, 0)
        .chars()
        .skip(usize::from(a.cols) + 1)
        .collect();
    assert!(after.trim().is_empty());
}

#[test]
fn tabs_are_drawn_as_a_strip_with_the_active_one_marked() {
    let layout = Layout::new(
        Region::Tabs {
            active: 1,
            tabs: vec![Region::Surface(id("agent")), Region::Surface(id("shell"))],
        },
        id("shell"),
    )
    .unwrap();
    let solved = solve(&layout, Rect::new(0, 0, 40, 6), &LayoutStyle::default());
    let mut shell = ScreenModel::new(40, 5);
    shell.feed(b"in the shell");
    let mut buf = buffer(40, 6);
    let theme = Theme::default();
    paint(
        &mut buf,
        &solved,
        &|surface| (surface.as_str() == "shell").then_some(&shell),
        &|surface| surface.to_string().to_uppercase(),
        &theme,
    );
    assert!(
        symbols(&buf, 0).starts_with(" AGENT  SHELL "),
        "{:?}",
        symbols(&buf, 0)
    );
    assert_eq!(buf[(1, 0)].bg, theme.tab.bg.unwrap());
    assert_eq!(buf[(9, 0)].bg, theme.tab_active.bg.unwrap());
    assert_eq!(symbols(&buf, 1).trim(), "in the shell");
}

#[test]
fn a_squeezed_layout_paints_the_focused_screen_across_everything() {
    let layout = two_up();
    let solved = solve(&layout, Rect::new(0, 0, 25, 6), &LayoutStyle::default());
    assert!(solved.squeezed);
    let mut agent = ScreenModel::new(25, 6);
    agent.feed(b"alone");
    let mut buf = buffer(25, 6);
    let painted = paint(
        &mut buf,
        &solved,
        &|surface| (surface.as_str() == "agent").then_some(&agent),
        &|surface| surface.to_string(),
        &Theme::default(),
    );
    assert_eq!(symbols(&buf, 0).trim(), "alone");
    assert_eq!(painted.cursor, Some((5, 0)));
}
