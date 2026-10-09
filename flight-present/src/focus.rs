// SPDX-License-Identifier: MIT

use crate::{Rect, Solved};
use flight_state::SurfaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The next showing surface in reading order after `from` (or before it), wrapping round.
pub fn cycle(solved: &Solved, from: &SurfaceId, forward: bool) -> Option<SurfaceId> {
    let n = solved.tiles.len();
    let at = solved.tiles.iter().position(|t| &t.surface == from)?;
    let next = if forward {
        at.saturating_add(1) % n.max(1)
    } else {
        at.checked_sub(1).unwrap_or(n.saturating_sub(1))
    };
    solved.tiles.get(next).map(|t| t.surface.clone())
}

/// The showing surface next to `from` in `direction`: the nearest tile on that side that shares
/// part of an edge with it, the one sharing the most winning, then the one whose centre is
/// closest. `None` when `from` has nothing on that side.
pub fn neighbor(solved: &Solved, from: &SurfaceId, direction: Direction) -> Option<SurfaceId> {
    let here = solved.tile(from)?.area;
    solved
        .tiles
        .iter()
        .filter(|t| &t.surface != from)
        .filter_map(|t| {
            let (gap, shared) = edge(&here, &t.area, direction)?;
            Some((gap, std::cmp::Reverse(shared), distance(&here, &t.area), t))
        })
        .min_by_key(|(gap, shared, dist, _)| (*gap, *shared, *dist))
        .map(|(_, _, _, t)| t.surface.clone())
}

/// If `there` lies on the `direction` side of `here` and shares part of that edge: how far
/// apart the edges are, and how long the shared part is.
fn edge(here: &Rect, there: &Rect, direction: Direction) -> Option<(u16, u16)> {
    match direction {
        Direction::Right => (there.x >= here.right()).then(|| {
            (
                there.x.saturating_sub(here.right()),
                here.shared_rows(there),
            )
        }),
        Direction::Left => (there.right() <= here.x).then(|| {
            (
                here.x.saturating_sub(there.right()),
                here.shared_rows(there),
            )
        }),
        Direction::Down => (there.y >= here.bottom()).then(|| {
            (
                there.y.saturating_sub(here.bottom()),
                here.shared_cols(there),
            )
        }),
        Direction::Up => (there.bottom() <= here.y).then(|| {
            (
                here.y.saturating_sub(there.bottom()),
                here.shared_cols(there),
            )
        }),
    }
    .filter(|(_, shared)| *shared > 0)
}

fn distance(a: &Rect, b: &Rect) -> u64 {
    let (ax, ay) = a.center();
    let (bx, by) = b.center();
    u64::from(ax.abs_diff(bx)).saturating_add(u64::from(ay.abs_diff(by)))
}
