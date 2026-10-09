// SPDX-License-Identifier: MIT

use crate::{Axis, Divider, Layout, Rect, Region, Style, TabBar, Tile};
use flight_state::SurfaceId;

/// A layout laid out on a terminal of a given size: where each showing surface goes, the lines
/// between them, the tab strips, and which surfaces are not showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solved {
    pub tiles: Vec<Tile>,
    pub dividers: Vec<Divider>,
    pub tab_bars: Vec<TabBar>,
    /// Surfaces the layout names that have no tile (inactive tabs, or everything but the focus
    /// when the terminal is too small): they need no attachment.
    pub hidden: Vec<SurfaceId>,
    /// The terminal is too small to show every tile at the minimum size, so only the focused
    /// surface is shown, filling it. Growing the terminal brings the layout back as it was.
    pub squeezed: bool,
}

impl Solved {
    pub fn tile(&self, surface: &SurfaceId) -> Option<&Tile> {
        self.tiles.iter().find(|t| &t.surface == surface)
    }

    pub fn focused(&self) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.focused)
    }
}

/// Lay `layout` out on `area`. Deterministic: the same layout and area give the same answer,
/// and every cell of `area` belongs to exactly one tile, divider or tab strip.
pub fn solve(layout: &Layout, area: Rect, style: &Style) -> Solved {
    let mut out = Solved {
        tiles: Vec::new(),
        dividers: Vec::new(),
        tab_bars: Vec::new(),
        hidden: Vec::new(),
        squeezed: false,
    };
    if fits(layout.root(), area, style) {
        place(layout.root(), area, layout.focus(), style, &mut out);
    } else {
        out.squeezed = true;
        out.tiles.push(Tile {
            surface: layout.focus().clone(),
            area,
            focused: true,
        });
    }
    out.hidden = layout
        .surfaces()
        .into_iter()
        .filter(|s| out.tile(s).is_none())
        .cloned()
        .collect();
    out
}

/// The least a region needs in each direction.
fn need(region: &Region, style: &Style) -> (u32, u32) {
    match region {
        Region::Surface(_) => (u32::from(style.min_cols), u32::from(style.min_rows)),
        Region::Split { axis, children } => {
            let sizes: Vec<(u32, u32)> = children.iter().map(|c| need(&c.region, style)).collect();
            let dividers = u32::try_from(children.len().saturating_sub(1)).unwrap_or(u32::MAX);
            match axis {
                Axis::Across => (
                    sizes
                        .iter()
                        .map(|s| s.0)
                        .sum::<u32>()
                        .saturating_add(dividers),
                    sizes.iter().map(|s| s.1).max().unwrap_or(0),
                ),
                Axis::Down => (
                    sizes.iter().map(|s| s.0).max().unwrap_or(0),
                    sizes
                        .iter()
                        .map(|s| s.1)
                        .sum::<u32>()
                        .saturating_add(dividers),
                ),
            }
        }
        Region::Tabs { active, tabs } => {
            let (c, r) = tabs.get(*active).map_or((0, 0), |t| need(t, style));
            (c, r.saturating_add(1))
        }
    }
}

fn fits(region: &Region, area: Rect, style: &Style) -> bool {
    let (c, r) = need(region, style);
    u32::from(area.cols) >= c && u32::from(area.rows) >= r
}

fn place(region: &Region, area: Rect, focus: &SurfaceId, style: &Style, out: &mut Solved) {
    match region {
        Region::Surface(s) => out.tiles.push(Tile {
            surface: s.clone(),
            area,
            focused: s == focus,
        }),
        Region::Tabs { active, tabs } => {
            let bar = Rect::new(area.x, area.y, area.cols, area.rows.min(1));
            let body = Rect::new(
                area.x,
                area.y.saturating_add(1),
                area.cols,
                area.rows.saturating_sub(1),
            );
            out.tab_bars.push(TabBar {
                area: bar,
                tabs: tabs.iter().filter_map(first_surface).collect(),
                active: *active,
            });
            if let Some(t) = tabs.get(*active) {
                place(t, body, focus, style, out);
            }
        }
        Region::Split { axis, children } => {
            let count = u32::try_from(children.len()).unwrap_or(u32::MAX);
            let dividers = count.saturating_sub(1);
            let total = match axis {
                Axis::Across => u32::from(area.cols),
                Axis::Down => u32::from(area.rows),
            };
            let free = total.saturating_sub(dividers);
            let mins: Vec<u32> = children
                .iter()
                .map(|c| {
                    let (cols, rows) = need(&c.region, style);
                    match axis {
                        Axis::Across => cols,
                        Axis::Down => rows,
                    }
                })
                .collect();
            let sizes = share(
                free,
                children.iter().map(|c| u32::from(c.weight)).collect(),
                &mins,
            );
            let mut at = match axis {
                Axis::Across => area.x,
                Axis::Down => area.y,
            };
            for (i, (child, size)) in children.iter().zip(sizes).enumerate() {
                let size = u16::try_from(size).unwrap_or(u16::MAX);
                let tile = match axis {
                    Axis::Across => Rect::new(at, area.y, size, area.rows),
                    Axis::Down => Rect::new(area.x, at, area.cols, size),
                };
                place(&child.region, tile, focus, style, out);
                at = at.saturating_add(size);
                if i.saturating_add(1) < children.len() {
                    let line = match axis {
                        Axis::Across => Rect::new(at, area.y, 1, area.rows),
                        Axis::Down => Rect::new(area.x, at, area.cols, 1),
                    };
                    out.dividers.push(Divider {
                        area: line,
                        vertical: *axis == Axis::Across,
                    });
                    at = at.saturating_add(1);
                }
            }
        }
    }
}

fn first_surface(region: &Region) -> Option<SurfaceId> {
    let mut all = Vec::new();
    region.surfaces(&mut all);
    all.first().map(|s| (*s).clone())
}

/// `total` cells divided in proportion to `weights`, every cell used, no child below its
/// minimum. Each unclamped child gets the floor of its share and the cells left over go, one
/// each, to the largest remainders (earlier first). A child whose share would fall below its
/// minimum is given the minimum, and the rest are divided again among the others.
fn share(total: u32, weights: Vec<u32>, mins: &[u32]) -> Vec<u32> {
    let n = weights.len();
    let mut size: Vec<Option<u32>> = vec![None; n];
    loop {
        let fixed: u32 = size.iter().flatten().sum();
        let open: Vec<usize> = (0..n)
            .filter(|i| size.get(*i).is_some_and(Option::is_none))
            .collect();
        let rest = total.saturating_sub(fixed);
        let ws: Vec<u32> = open
            .iter()
            .map(|i| weights.get(*i).copied().unwrap_or(0))
            .collect();
        let parts = proportion(rest, &ws);
        let short: Vec<usize> = open
            .iter()
            .zip(&parts)
            .filter(|(i, got)| **got < mins.get(**i).copied().unwrap_or(0))
            .map(|(i, _)| *i)
            .collect();
        if short.is_empty() {
            for (i, got) in open.iter().zip(parts) {
                if let Some(slot) = size.get_mut(*i) {
                    *slot = Some(got);
                }
            }
            break;
        }
        for i in short {
            if let Some(slot) = size.get_mut(i) {
                *slot = Some(mins.get(i).copied().unwrap_or(0));
            }
        }
        if size.iter().all(Option::is_some) {
            break;
        }
    }
    size.into_iter().map(|s| s.unwrap_or(0)).collect()
}

fn proportion(total: u32, weights: &[u32]) -> Vec<u32> {
    let sum: u64 = weights.iter().map(|w| u64::from(*w)).sum();
    if sum == 0 || weights.is_empty() {
        return vec![0; weights.len()];
    }
    let mut base: Vec<u32> = Vec::with_capacity(weights.len());
    let mut remainders: Vec<(u64, usize)> = Vec::with_capacity(weights.len());
    for (i, w) in weights.iter().enumerate() {
        let scaled = u64::from(total).saturating_mul(u64::from(*w));
        base.push(u32::try_from(scaled / sum).unwrap_or(u32::MAX));
        remainders.push((scaled % sum, i));
    }
    let used: u32 = base.iter().sum();
    let mut left = total.saturating_sub(used);
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in remainders {
        if left == 0 {
            break;
        }
        if let Some(b) = base.get_mut(i) {
            *b = b.saturating_add(1);
            left = left.saturating_sub(1);
        }
    }
    base
}
