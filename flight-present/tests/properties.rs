// SPDX-License-Identifier: MIT

//! Properties over many generated layouts and terminal sizes: whatever the tree, the solved
//! layout tiles the terminal exactly, and no edit breaks an invariant or touches more than the
//! surface it was about. A fixed generator, so a failure repeats.

use flight_present::{
    cycle, neighbor, saved, solve, Axis, Child, Direction, Layout, Placement, Rect, Region, Style,
};
use flight_state::SurfaceId;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

struct Gen {
    rng: Rng,
    n: usize,
}

impl Gen {
    fn surface(&mut self) -> SurfaceId {
        self.n += 1;
        SurfaceId::new(format!("s-{}", self.n))
    }

    fn region(&mut self, depth: usize) -> Region {
        let kind = if depth >= 4 { 0 } else { self.rng.below(5) };
        match kind {
            0 | 1 => Region::Surface(self.surface()),
            2 | 3 => {
                let count = 2 + self.rng.below(3) as usize;
                Region::Split {
                    axis: if self.rng.below(2) == 0 {
                        Axis::Across
                    } else {
                        Axis::Down
                    },
                    children: (0..count)
                        .map(|_| {
                            let w = 1 + self.rng.below(8) as u16;
                            Child::new(w, self.region(depth + 1))
                        })
                        .collect(),
                }
            }
            _ => {
                let count = 2 + self.rng.below(2) as usize;
                let tabs: Vec<Region> = (0..count).map(|_| self.region(depth + 1)).collect();
                Region::Tabs {
                    active: self.rng.below(count as u64) as usize,
                    tabs,
                }
            }
        }
    }

    fn layout(&mut self) -> Layout {
        loop {
            self.n = 0;
            let root = self.region(0);
            let mut shown = Vec::new();
            root.visible(&mut shown);
            let focus = shown[self.rng.below(shown.len() as u64) as usize].clone();
            if let Ok(l) = Layout::new(root, focus) {
                return l;
            }
        }
    }
}

fn sizes(rng: &mut Rng) -> Rect {
    Rect::new(0, 0, 1 + rng.below(240) as u16, 1 + rng.below(80) as u16)
}

#[test]
fn the_solved_layout_tiles_the_terminal_exactly() {
    let mut g = Gen {
        rng: Rng(0x9E37_79B9_7F4A_7C15),
        n: 0,
    };
    for _ in 0..2000 {
        let l = g.layout();
        let area = sizes(&mut g.rng);
        let style = Style::default();
        let s = solve(&l, area, &style);
        // Every cell belongs to exactly one tile, divider or tab strip.
        let mut rects: Vec<Rect> = s.tiles.iter().map(|t| t.area).collect();
        rects.extend(s.dividers.iter().map(|d| d.area));
        rects.extend(s.tab_bars.iter().map(|b| b.area));
        let total: u32 = rects.iter().map(Rect::cells).sum();
        assert_eq!(total, area.cells(), "{l:?} on {area:?}");
        for (i, a) in rects.iter().enumerate() {
            assert!(a.x >= area.x && a.right() <= area.right());
            assert!(a.y >= area.y && a.bottom() <= area.bottom());
            for b in rects.iter().skip(i + 1) {
                assert!(!a.intersects(b), "{a:?} overlaps {b:?} in {l:?}");
            }
        }
        // Unless squeezed, every tile is usable; squeezed means exactly the focus, whole.
        if s.squeezed {
            assert_eq!(s.tiles.len(), 1);
            assert_eq!(s.tiles[0].area, area);
        } else {
            for t in &s.tiles {
                assert!(
                    t.area.cols >= style.min_cols && t.area.rows >= style.min_rows,
                    "{t:?}"
                );
            }
        }
        // The keyboard is on exactly one showing tile; shown and hidden account for every
        // surface, once.
        assert_eq!(s.tiles.iter().filter(|t| t.focused).count(), 1);
        assert_eq!(s.tiles.len() + s.hidden.len(), l.surfaces().len());
        // Deterministic.
        assert_eq!(s, solve(&l, area, &style));
    }
}

#[test]
fn focus_can_reach_every_showing_tile_by_cycling() {
    let mut g = Gen { rng: Rng(42), n: 0 };
    for _ in 0..500 {
        let l = g.layout();
        let s = solve(&l, Rect::new(0, 0, 200, 70), &Style::default());
        if s.squeezed {
            continue;
        }
        let mut at = s.tiles[0].surface.clone();
        let mut seen = vec![at.clone()];
        for _ in 0..s.tiles.len() {
            at = cycle(&s, &at, true).unwrap();
            if !seen.contains(&at) {
                seen.push(at.clone());
            }
        }
        assert_eq!(seen.len(), s.tiles.len());
        // A neighbour is a different showing tile, and going back finds a way home.
        for t in &s.tiles {
            for d in [
                Direction::Left,
                Direction::Right,
                Direction::Up,
                Direction::Down,
            ] {
                if let Some(n) = neighbor(&s, &t.surface, d) {
                    assert_ne!(n, t.surface);
                    assert!(s.tile(&n).is_some());
                }
            }
        }
    }
}

#[test]
fn edits_keep_the_invariants_and_touch_only_what_they_name() {
    let mut g = Gen { rng: Rng(7), n: 0 };
    let mut next = 1000u32;
    for _ in 0..1500 {
        let l = g.layout();
        let all: Vec<SurfaceId> = l.surfaces().into_iter().cloned().collect();
        let pick = all[g.rng.below(all.len() as u64) as usize].clone();
        next += 1;
        let fresh = SurfaceId::new(format!("n-{next}"));
        // Split: exactly one more surface, the old ones all still named.
        let axis = if g.rng.below(2) == 0 {
            Axis::Across
        } else {
            Axis::Down
        };
        let placement = if g.rng.below(2) == 0 {
            Placement::Before
        } else {
            Placement::After
        };
        if let Ok(split) = l.split(&pick, axis, fresh.clone(), placement) {
            assert_eq!(split.surfaces().len(), all.len() + 1);
            assert!(all.iter().all(|s| split.contains(s)) && split.contains(&fresh));
            assert_eq!(split.focus(), &fresh);
            // And taking it away again restores the same set of surfaces.
            let undone = split.remove(&fresh).unwrap();
            let mut a: Vec<_> = undone.surfaces().into_iter().cloned().collect();
            let mut b = all.clone();
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
        if let Ok(tabbed) = l.add_tab(&pick, SurfaceId::new(format!("t-{next}"))) {
            assert_eq!(tabbed.surfaces().len(), all.len() + 1);
            assert!(tabbed.visible().contains(&tabbed.focus()));
        }
        // Remove: exactly that surface goes, the keyboard stays on something showing.
        if all.len() > 1 {
            let smaller = l.remove(&pick).unwrap();
            assert_eq!(smaller.surfaces().len(), all.len() - 1);
            assert!(!smaller.contains(&pick));
            assert!(smaller.visible().contains(&smaller.focus()));
        }
        // Resize and swap change places and shares, never the set of surfaces.
        let resized = l.resize(&pick, axis, g.rng.below(41) as i32 - 20).unwrap();
        let swapped = l.swap(&pick, &all[0]).unwrap();
        for other in [&resized, &swapped] {
            let mut a: Vec<_> = other.surfaces().into_iter().cloned().collect();
            let mut b = all.clone();
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
        // Replacing keeps the shape: the same number of surfaces, the new name where the old was.
        let renamed = l
            .replace(&pick, SurfaceId::new(format!("r-{next}")))
            .unwrap();
        assert_eq!(renamed.surfaces().len(), all.len());
        assert!(!renamed.contains(&pick));
        // Stepping a tab set keeps the keyboard on something showing.
        let stepped = l.step_tab(g.rng.below(2) == 0);
        assert!(stepped.visible().contains(&stepped.focus()));
        assert_eq!(stepped.surfaces().len(), all.len());
        // Focusing anything shows it.
        let focused = l.focus_on(&pick).unwrap();
        assert!(focused.visible().contains(&&pick));
    }
}

#[test]
fn what_is_saved_is_what_is_read_back() {
    let mut g = Gen { rng: Rng(99), n: 0 };
    for _ in 0..500 {
        let l = g.layout();
        let text = saved::to_toml(&l).unwrap();
        assert!(text.len() <= saved::MAX_BYTES);
        assert_eq!(saved::from_toml(&text, |_| true).unwrap(), l);
        // Fitting to fewer surfaces never invents one and always leaves a valid layout.
        let keep: Vec<SurfaceId> = l.surfaces().into_iter().step_by(2).cloned().collect();
        match saved::from_toml(&text, |s| keep.contains(s)) {
            Ok(fitted) => {
                assert!(fitted.surfaces().iter().all(|s| keep.contains(s)));
                assert!(fitted.visible().contains(&fitted.focus()));
            }
            Err(e) => assert_eq!(e, flight_present::LayoutError::Empty),
        }
    }
}
