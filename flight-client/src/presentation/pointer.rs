// SPDX-License-Identifier: MIT

use super::mouse_event::{Kind, MouseEvent};
use crate::screens::Modes;
use flight_present::{Solved, Tile};
use flight_state::SurfaceId;

/// What a mouse event comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Routed {
    /// A click on a tile that did not have the keyboard: it has it now. The click goes no
    /// further, so it does not also do something to the program in that tile.
    Focus(SurfaceId),
    /// The report to write to this surface's program.
    Send(SurfaceId, Vec<u8>),
}

/// Where a button press began, until the button is let go.
struct Grab {
    surface: SurfaceId,
    /// Whether the program is told: not when the press only moved the keyboard.
    forward: bool,
}

/// Decides which surface a mouse event is for. A press belongs to the tile under it; the drag
/// and the release that follow belong to that same tile, wherever the pointer has gone, so a
/// selection that leaves its tile still ends in it. Wheel turns and movement go to the tile
/// under the pointer.
#[derive(Default)]
pub(super) struct Pointer {
    grab: Option<Grab>,
}

impl Pointer {
    /// `modes` gives the modes of the screen of a surface.
    pub(super) fn route(
        &mut self,
        event: MouseEvent,
        solved: &Solved,
        modes: &dyn Fn(&SurfaceId) -> Option<Modes>,
    ) -> Option<Routed> {
        let kind = event.kind();
        if matches!(kind, Kind::Release | Kind::Drag) {
            let (surface, forward) = {
                let grab = self.grab.as_ref()?;
                (grab.surface.clone(), grab.forward)
            };
            if kind == Kind::Release {
                self.grab = None;
            }
            let tile = solved.tile(&surface);
            if tile.is_none() {
                self.grab = None;
            }
            return if forward {
                send(event, tile?, modes)
            } else {
                None
            };
        }
        let tile = tile_at(solved, &event)?;
        match kind {
            Kind::Press => {
                self.grab = Some(Grab {
                    surface: tile.surface.clone(),
                    forward: tile.focused,
                });
                if tile.focused {
                    send(event, tile, modes)
                } else {
                    Some(Routed::Focus(tile.surface.clone()))
                }
            }
            _ => send(event, tile, modes),
        }
    }
}

fn tile_at<'a>(solved: &'a Solved, event: &MouseEvent) -> Option<&'a Tile> {
    solved
        .tiles
        .iter()
        .find(|t| t.area.contains(event.col, event.row))
}

fn send(
    event: MouseEvent,
    tile: &Tile,
    modes: &dyn Fn(&SurfaceId) -> Option<Modes>,
) -> Option<Routed> {
    let modes = modes(&tile.surface)?;
    if !event.wanted_by(modes.mouse) {
        return None;
    }
    let a = &tile.area;
    let col = event.col.saturating_sub(a.x).min(a.cols.saturating_sub(1));
    let row = event.row.saturating_sub(a.y).min(a.rows.saturating_sub(1));
    let report = event.report(col, row, modes.mouse_encoding)?;
    Some(Routed::Send(tile.surface.clone(), report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::{MouseEncoding, MouseMode};
    use flight_present::Rect;

    fn tile(name: &str, area: Rect, focused: bool) -> Tile {
        Tile {
            surface: SurfaceId::new(name),
            area,
            focused,
        }
    }

    /// Two tiles side by side on a 21x5 terminal: `a` (focused) at columns 0-9, a divider, `b`.
    fn solved() -> Solved {
        Solved {
            tiles: vec![
                tile("a", Rect::new(0, 0, 10, 5), true),
                tile("b", Rect::new(11, 0, 10, 5), false),
            ],
            dividers: Vec::new(),
            tab_bars: Vec::new(),
            hidden: Vec::new(),
            squeezed: false,
        }
    }

    fn modes(mouse: MouseMode) -> impl Fn(&SurfaceId) -> Option<Modes> {
        move |_| {
            Some(Modes {
                alternate_screen: true,
                bracketed_paste: false,
                application_cursor_keys: false,
                mouse,
                mouse_encoding: MouseEncoding::Sgr,
            })
        }
    }

    fn at(code: u16, col: u16, row: u16, release: bool) -> MouseEvent {
        MouseEvent {
            code,
            col,
            row,
            release,
        }
    }

    #[test]
    fn a_press_in_the_focused_tile_is_told_to_its_program_in_the_tiles_own_columns() {
        let mut p = Pointer::default();
        let got = p.route(
            at(0, 3, 2, false),
            &solved(),
            &modes(MouseMode::PressRelease),
        );
        assert_eq!(
            got,
            Some(Routed::Send(SurfaceId::new("a"), b"\x1b[<0;4;3M".to_vec()))
        );
    }

    #[test]
    fn a_press_in_another_tile_moves_the_keyboard_and_goes_no_further() {
        let mut p = Pointer::default();
        let m = modes(MouseMode::Motion);
        assert_eq!(
            p.route(at(0, 12, 1, false), &solved(), &m),
            Some(Routed::Focus(SurfaceId::new("b")))
        );
        assert_eq!(p.route(at(32, 13, 1, false), &solved(), &m), None);
        assert_eq!(p.route(at(0, 13, 1, true), &solved(), &m), None);
    }

    #[test]
    fn a_drag_and_its_release_stay_with_the_tile_the_press_began_in() {
        let mut p = Pointer::default();
        let m = modes(MouseMode::Drag);
        p.route(at(0, 8, 1, false), &solved(), &m);
        let drag = p.route(at(32, 15, 9, false), &solved(), &m);
        assert_eq!(
            drag,
            Some(Routed::Send(
                SurfaceId::new("a"),
                b"\x1b[<32;10;5M".to_vec()
            ))
        );
        let up = p.route(at(0, 15, 9, true), &solved(), &m);
        assert_eq!(
            up,
            Some(Routed::Send(SurfaceId::new("a"), b"\x1b[<0;10;5m".to_vec()))
        );
        assert_eq!(p.route(at(32, 5, 1, false), &solved(), &m), None);
    }

    #[test]
    fn the_wheel_goes_to_the_tile_under_the_pointer_and_what_is_not_a_tile_to_nobody() {
        let mut p = Pointer::default();
        let m = modes(MouseMode::PressRelease);
        assert_eq!(
            p.route(at(64, 14, 0, false), &solved(), &m),
            Some(Routed::Send(SurfaceId::new("b"), b"\x1b[<64;4;1M".to_vec()))
        );
        assert_eq!(p.route(at(0, 10, 2, false), &solved(), &m), None);
        assert_eq!(
            p.route(at(64, 14, 0, false), &solved(), &modes(MouseMode::None)),
            None
        );
    }
}
