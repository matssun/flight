// SPDX-License-Identifier: MIT

use super::cell::Colour;
use super::{Geometry, Modes, MouseMode, ScreenModel};
use flight_present::{Divider, Rect, Solved, TabBar, Tile};
use flight_state::SurfaceId;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

/// How the parts that are not a surface look.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub divider: Style,
    /// A divider next to the surface that has the keyboard.
    pub divider_focused: Style,
    pub tab: Style,
    pub tab_active: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            divider: Style::default().fg(Color::DarkGray),
            divider_focused: Style::default().fg(Color::Cyan),
            tab: Style::default().fg(Color::Gray).bg(Color::DarkGray),
            tab_active: Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// What the caller needs after a frame is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Painted {
    /// Where to put the real cursor: the focused surface's cursor, if it shows one inside its
    /// tile. (Column, row) on the terminal.
    pub cursor: Option<(u16, u16)>,
    /// The modes the real terminal has to be in: those of the focused surface's screen, if it
    /// has one. The focused surface is the one the keyboard (and so the cursor and the key
    /// reporting) belongs to; no other surface's modes reach the real terminal.
    pub modes: Option<Modes>,
    /// What the real terminal has to report of the mouse: what the focused program asked for,
    /// and at least presses and releases while there is more than one place to click (a tile
    /// to move the keyboard to, a tab), which the program has not asked for.
    pub pointer: MouseMode,
}

/// Draw a solved layout into `buf`: every showing surface's screen in its tile, the lines
/// between tiles, the tab strips. `screen` gives the model of a surface (a tile whose surface
/// has none yet is left blank), `label` the text of a tab.
pub fn paint<'a>(
    buf: &mut Buffer,
    solved: &Solved,
    screen: &dyn Fn(&SurfaceId) -> Option<&'a ScreenModel>,
    label: &dyn Fn(&SurfaceId) -> String,
    theme: &Theme,
) -> Painted {
    let focused_area = solved.focused().map(|t| t.area);
    let mut cursor = None;
    let mut modes = None;
    for tile in &solved.tiles {
        let model = screen(&tile.surface);
        paint_tile(buf, tile, model);
        if tile.focused {
            cursor = model.and_then(|m| tile_cursor(tile, m));
            modes = model.map(ScreenModel::modes);
        }
    }
    for divider in &solved.dividers {
        paint_divider(buf, divider, focused_area.as_ref(), theme);
    }
    for bar in &solved.tab_bars {
        paint_tabs(buf, bar, label, theme);
    }
    let several = solved.tiles.len() > 1 || !solved.tab_bars.is_empty();
    let asked = modes.map_or(MouseMode::None, |m| m.mouse);
    let pointer = if several {
        asked.max(MouseMode::PressRelease)
    } else {
        asked
    };
    Painted {
        cursor,
        modes,
        pointer,
    }
}

fn tile_cursor(tile: &Tile, model: &ScreenModel) -> Option<(u16, u16)> {
    let (col, row) = model.cursor()?;
    (col < tile.area.cols && row < tile.area.rows).then(|| {
        (
            tile.area.x.saturating_add(col),
            tile.area.y.saturating_add(row),
        )
    })
}

/// What a tile too small for any screen shows, in its first cell.
const PLACEHOLDER: &str = "…";

fn paint_tile(buf: &mut Buffer, tile: &Tile, model: Option<&ScreenModel>) {
    let too_small = Geometry::new(tile.area.cols, tile.area.rows).is_err();
    for row in 0..tile.area.rows {
        for col in 0..tile.area.cols {
            let (x, y) = (
                buf.area.x.saturating_add(tile.area.x).saturating_add(col),
                buf.area.y.saturating_add(tile.area.y).saturating_add(row),
            );
            if x >= buf.area.right() || y >= buf.area.bottom() {
                continue;
            }
            let target = &mut buf[(x, y)];
            target.reset();
            if too_small {
                if col == 0 && row == 0 {
                    target.set_symbol(PLACEHOLDER);
                }
                continue;
            }
            let Some(cell) = model.and_then(|m| m.cell(col, row)) else {
                continue;
            };
            if cell.continuation {
                continue;
            }
            // A wide character at the last column would spill into the next tile.
            if cell.wide && col.saturating_add(1) >= tile.area.cols {
                continue;
            }
            if !cell.text.is_empty() {
                target.set_symbol(cell.text);
            }
            target.set_style(style_of(&cell));
        }
    }
}

fn style_of(cell: &super::CellView<'_>) -> Style {
    let mut style = Style::default().fg(colour(cell.fg)).bg(colour(cell.bg));
    if cell.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn colour(c: Colour) -> Color {
    match c {
        Colour::Default => Color::Reset,
        Colour::Indexed(i) => Color::Indexed(i),
        Colour::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn paint_divider(buf: &mut Buffer, divider: &Divider, focused: Option<&Rect>, theme: &Theme) {
    let next_to_focus = focused.is_some_and(|f| touches(&divider.area, f));
    let style = if next_to_focus {
        theme.divider_focused
    } else {
        theme.divider
    };
    let symbol = if divider.vertical { "│" } else { "─" };
    for row in 0..divider.area.rows {
        for col in 0..divider.area.cols {
            let (x, y) = (
                buf.area
                    .x
                    .saturating_add(divider.area.x)
                    .saturating_add(col),
                buf.area
                    .y
                    .saturating_add(divider.area.y)
                    .saturating_add(row),
            );
            if x < buf.area.right() && y < buf.area.bottom() {
                let cell = &mut buf[(x, y)];
                cell.reset();
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

/// A line is next to a tile when it runs along one of its edges.
fn touches(line: &Rect, tile: &Rect) -> bool {
    let along_side =
        (line.x == tile.right() || line.right() == tile.x) && line.shared_rows(tile) > 0;
    let along_end =
        (line.y == tile.bottom() || line.bottom() == tile.y) && line.shared_cols(tile) > 0;
    along_side || along_end
}

fn paint_tabs(buf: &mut Buffer, bar: &TabBar, label: &dyn Fn(&SurfaceId) -> String, theme: &Theme) {
    let y = buf.area.y.saturating_add(bar.area.y);
    let mut x = buf.area.x.saturating_add(bar.area.x);
    let end = buf
        .area
        .x
        .saturating_add(bar.area.right())
        .min(buf.area.right());
    for col in x..end {
        let cell = &mut buf[(col, y)];
        cell.reset();
        cell.set_style(theme.tab);
    }
    for (i, surface) in bar.tabs.iter().enumerate() {
        let style = if i == bar.active {
            theme.tab_active
        } else {
            theme.tab
        };
        let text = format!(" {} ", label(surface));
        for ch in text.chars() {
            if x >= end {
                return;
            }
            let cell = &mut buf[(x, y)];
            cell.set_symbol(ch.encode_utf8(&mut [0u8; 4]))
                .set_style(style);
            x = x.saturating_add(1);
        }
    }
}
