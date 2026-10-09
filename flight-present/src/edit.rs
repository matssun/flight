// SPDX-License-Identifier: MIT

//! Changing a layout. Every edit returns a new layout that satisfies the invariants, or says
//! why not; none touches a surface.

use crate::layout::MAX_WEIGHT;
use crate::{Axis, Child, Layout, LayoutError, Region};
use flight_state::SurfaceId;

/// Where a new surface goes relative to the one it is split from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Before,
    After,
}

/// Shares are scaled to this total before one is nudged, so a nudge is a few percent.
const NORMAL_TOTAL: u32 = 100;

impl Layout {
    /// Show `new` next to `target`, taking half of what `target` had. The new surface has the
    /// focus.
    pub fn split(
        &self,
        target: &SurfaceId,
        axis: Axis,
        new: SurfaceId,
        placement: Placement,
    ) -> Result<Layout, LayoutError> {
        if !self.contains(target) {
            return Err(LayoutError::Unknown(target.to_string()));
        }
        if self.contains(&new) {
            return Err(LayoutError::Duplicate(new.to_string()));
        }
        let root = split_in(&self.root, target, axis, &new, placement);
        let layout = Layout { root, focus: new };
        layout.check()?;
        Ok(layout)
    }

    /// Show `new` as another tab where `target` is, and make it the one that shows.
    pub fn add_tab(&self, target: &SurfaceId, new: SurfaceId) -> Result<Layout, LayoutError> {
        if !self.contains(target) {
            return Err(LayoutError::Unknown(target.to_string()));
        }
        if self.contains(&new) {
            return Err(LayoutError::Duplicate(new.to_string()));
        }
        let root = tab_in(&self.root, target, &new);
        let layout = Layout { root, focus: new };
        layout.check()?;
        Ok(layout)
    }

    /// Stop showing `surface`. The surface itself is not touched. The focus moves to the
    /// neighbour that takes its place if it had it. Fails if it is the last one.
    pub fn remove(&self, surface: &SurfaceId) -> Result<Layout, LayoutError> {
        if !self.contains(surface) {
            return Err(LayoutError::Unknown(surface.to_string()));
        }
        let before = self.visible_ids();
        let Some(root) = remove_in(&self.root, surface) else {
            return Err(LayoutError::Empty);
        };
        let mut layout = Layout {
            root,
            focus: self.focus.clone(),
        };
        if &self.focus == surface || !layout.visible().contains(&&self.focus) {
            layout.focus = heir(&before, surface, &layout);
        }
        layout.check()?;
        Ok(layout)
    }

    /// Keep only the surfaces `keep` approves (a saved layout meeting the surfaces that exist
    /// now). `None` when nothing is left.
    pub fn retain(&self, keep: impl Fn(&SurfaceId) -> bool) -> Option<Layout> {
        let mut layout = self.clone();
        let gone: Vec<SurfaceId> = self
            .surfaces()
            .into_iter()
            .filter(|s| !keep(s))
            .cloned()
            .collect();
        for s in gone {
            layout = layout.remove(&s).ok()?;
        }
        Some(layout)
    }

    /// Give the keyboard to `surface`, showing its tab if it was hidden in one.
    pub fn focus_on(&self, surface: &SurfaceId) -> Result<Layout, LayoutError> {
        if !self.contains(surface) {
            return Err(LayoutError::Unknown(surface.to_string()));
        }
        Ok(Layout {
            root: reveal(&self.root, surface),
            focus: surface.clone(),
        })
    }

    /// Exchange the places of two surfaces.
    pub fn swap(&self, a: &SurfaceId, b: &SurfaceId) -> Result<Layout, LayoutError> {
        for s in [a, b] {
            if !self.contains(s) {
                return Err(LayoutError::Unknown(s.to_string()));
            }
        }
        // The surface with the keyboard may have moved into a tab that is not showing: show it.
        let root = reveal(&swap_in(&self.root, a, b), &self.focus);
        let layout = Layout {
            root,
            focus: self.focus.clone(),
        };
        layout.check()?;
        Ok(layout)
    }

    /// Make `surface` larger (positive `by`) or smaller along `axis`, at the expense of, or to
    /// the benefit of, its neighbour in the nearest split on that axis. No share goes below 1.
    pub fn resize(&self, surface: &SurfaceId, axis: Axis, by: i32) -> Result<Layout, LayoutError> {
        if !self.contains(surface) {
            return Err(LayoutError::Unknown(surface.to_string()));
        }
        let mut root = self.root.clone();
        nudge(&mut root, surface, axis, by);
        let layout = Layout {
            root,
            focus: self.focus.clone(),
        };
        layout.check()?;
        Ok(layout)
    }

    /// Show `new` where `old` is, in the same place and with the same share. If `old` had the
    /// keyboard, `new` has it. `old` is no longer named, which stops showing it and nothing else.
    pub fn replace(&self, old: &SurfaceId, new: SurfaceId) -> Result<Layout, LayoutError> {
        if !self.contains(old) {
            return Err(LayoutError::Unknown(old.to_string()));
        }
        if self.contains(&new) {
            return Err(LayoutError::Duplicate(new.to_string()));
        }
        let root = swap_in(&self.root, old, &new);
        let focus = if &self.focus == old {
            new
        } else {
            self.focus.clone()
        };
        let layout = Layout { root, focus };
        layout.check()?;
        Ok(layout)
    }

    /// Show the next (or previous) tab of the tab set the focused surface is in, wrapping
    /// round, and give the keyboard to its first showing surface. No tab set: no change.
    pub fn step_tab(&self, forward: bool) -> Layout {
        let mut root = self.root.clone();
        let Some(first) = step_in(&mut root, &self.focus, forward) else {
            return self.clone();
        };
        let layout = Layout { root, focus: first };
        if layout.check().is_ok() {
            layout
        } else {
            self.clone()
        }
    }

    fn visible_ids(&self) -> Vec<SurfaceId> {
        self.visible().into_iter().cloned().collect()
    }
}

fn leaf(region: &Region, surface: &SurfaceId) -> bool {
    matches!(region, Region::Surface(s) if s == surface)
}

fn contains(region: &Region, surface: &SurfaceId) -> bool {
    let mut all = Vec::new();
    region.surfaces(&mut all);
    all.contains(&surface)
}

fn split_in(
    region: &Region,
    target: &SurfaceId,
    axis: Axis,
    new: &SurfaceId,
    placement: Placement,
) -> Region {
    match region {
        Region::Surface(s) if s == target => {
            let old = Child::new(1, Region::Surface(s.clone()));
            let added = Child::new(1, Region::Surface(new.clone()));
            let children = match placement {
                Placement::Before => vec![added, old],
                Placement::After => vec![old, added],
            };
            Region::Split { axis, children }
        }
        Region::Surface(_) => region.clone(),
        Region::Split {
            axis: here,
            children,
        } => {
            // Next to the target in a split that already runs the same way: one more sibling,
            // not another level.
            if *here == axis {
                if let Some(i) = children.iter().position(|c| leaf(&c.region, target)) {
                    let mut out = children.clone();
                    let weight = out.get(i).map_or(1, |c| c.weight);
                    let at = match placement {
                        Placement::Before => i,
                        Placement::After => i.saturating_add(1),
                    };
                    out.insert(at, Child::new(weight, Region::Surface(new.clone())));
                    return Region::Split {
                        axis: *here,
                        children: out,
                    };
                }
            }
            Region::Split {
                axis: *here,
                children: children
                    .iter()
                    .map(|c| {
                        if contains(&c.region, target) {
                            Child::new(c.weight, split_in(&c.region, target, axis, new, placement))
                        } else {
                            c.clone()
                        }
                    })
                    .collect(),
            }
        }
        Region::Tabs { active, tabs } => Region::Tabs {
            active: *active,
            tabs: tabs
                .iter()
                .map(|t| {
                    if contains(t, target) {
                        split_in(t, target, axis, new, placement)
                    } else {
                        t.clone()
                    }
                })
                .collect(),
        },
    }
}

fn tab_in(region: &Region, target: &SurfaceId, new: &SurfaceId) -> Region {
    match region {
        Region::Surface(s) if s == target => Region::Tabs {
            active: 1,
            tabs: vec![Region::Surface(s.clone()), Region::Surface(new.clone())],
        },
        Region::Surface(_) => region.clone(),
        Region::Split { axis, children } => Region::Split {
            axis: *axis,
            children: children
                .iter()
                .map(|c| {
                    if contains(&c.region, target) {
                        Child::new(c.weight, tab_in(&c.region, target, new))
                    } else {
                        c.clone()
                    }
                })
                .collect(),
        },
        Region::Tabs { active, tabs } => {
            if tabs.iter().any(|t| leaf(t, target)) {
                let mut out = tabs.clone();
                out.push(Region::Surface(new.clone()));
                return Region::Tabs {
                    active: out.len().saturating_sub(1),
                    tabs: out,
                };
            }
            Region::Tabs {
                active: *active,
                tabs: tabs
                    .iter()
                    .map(|t| {
                        if contains(t, target) {
                            tab_in(t, target, new)
                        } else {
                            t.clone()
                        }
                    })
                    .collect(),
            }
        }
    }
}

/// The region without `surface`, collapsing what that leaves with one child. `None` when
/// nothing is left of it.
fn remove_in(region: &Region, surface: &SurfaceId) -> Option<Region> {
    match region {
        Region::Surface(s) if s == surface => None,
        Region::Surface(_) => Some(region.clone()),
        Region::Split { axis, children } => {
            let kept: Vec<Child> = children
                .iter()
                .filter_map(|c| remove_in(&c.region, surface).map(|r| Child::new(c.weight, r)))
                .collect();
            match kept.len() {
                0 => None,
                1 => kept.into_iter().next().map(|c| c.region),
                _ => Some(Region::Split {
                    axis: *axis,
                    children: kept,
                }),
            }
        }
        Region::Tabs { active, tabs } => {
            let mut kept: Vec<Region> = Vec::new();
            let mut active_at: Option<usize> = None;
            let mut before_active = 0usize;
            for (i, t) in tabs.iter().enumerate() {
                let Some(r) = remove_in(t, surface) else {
                    continue;
                };
                if i == *active {
                    active_at = Some(kept.len());
                }
                if i < *active {
                    before_active = before_active.saturating_add(1);
                }
                kept.push(r);
            }
            // If the tab that was showing went, the one before it shows (or the first).
            let shows = active_at.unwrap_or_else(|| before_active.saturating_sub(1));
            match kept.len() {
                0 => None,
                1 => kept.into_iter().next(),
                n => Some(Region::Tabs {
                    active: shows.min(n.saturating_sub(1)),
                    tabs: kept,
                }),
            }
        }
    }
}

/// Who gets the keyboard when `gone` leaves: the surface that was showing just before it, else
/// the first one showing now.
fn heir(before: &[SurfaceId], gone: &SurfaceId, layout: &Layout) -> SurfaceId {
    let now = layout.visible();
    let at = before.iter().position(|s| s == gone).unwrap_or(0);
    before
        .iter()
        .take(at)
        .rev()
        .chain(before.iter().skip(at.saturating_add(1)))
        .find(|s| now.contains(s))
        .or_else(|| now.first().copied())
        .cloned()
        .unwrap_or_else(|| gone.clone())
}

fn reveal(region: &Region, surface: &SurfaceId) -> Region {
    match region {
        Region::Surface(_) => region.clone(),
        Region::Split { axis, children } => Region::Split {
            axis: *axis,
            children: children
                .iter()
                .map(|c| Child::new(c.weight, reveal(&c.region, surface)))
                .collect(),
        },
        Region::Tabs { active, tabs } => Region::Tabs {
            active: tabs
                .iter()
                .position(|t| contains(t, surface))
                .unwrap_or(*active),
            tabs: tabs.iter().map(|t| reveal(t, surface)).collect(),
        },
    }
}

fn swap_in(region: &Region, a: &SurfaceId, b: &SurfaceId) -> Region {
    match region {
        Region::Surface(s) if s == a => Region::Surface(b.clone()),
        Region::Surface(s) if s == b => Region::Surface(a.clone()),
        Region::Surface(_) => region.clone(),
        Region::Split { axis, children } => Region::Split {
            axis: *axis,
            children: children
                .iter()
                .map(|c| Child::new(c.weight, swap_in(&c.region, a, b)))
                .collect(),
        },
        Region::Tabs { active, tabs } => Region::Tabs {
            active: *active,
            tabs: tabs.iter().map(|t| swap_in(t, a, b)).collect(),
        },
    }
}

/// Move share between `surface`'s branch of the nearest split on `axis` and its neighbour.
/// Returns whether this region (or something below it) did it.
fn nudge(region: &mut Region, surface: &SurfaceId, axis: Axis, by: i32) -> bool {
    match region {
        Region::Surface(_) => false,
        Region::Tabs { tabs, .. } => tabs
            .iter_mut()
            .any(|t| contains(t, surface) && nudge(t, surface, axis, by)),
        Region::Split {
            axis: here,
            children,
        } => {
            let Some(i) = children.iter().position(|c| contains(&c.region, surface)) else {
                return false;
            };
            // The nearest split below that runs the right way gets the first go.
            if let Some(c) = children.get_mut(i) {
                if nudge(&mut c.region, surface, axis, by) {
                    return true;
                }
            }
            if *here != axis || children.len() < 2 {
                return false;
            }
            normalize(children);
            let other = if i.saturating_add(1) < children.len() {
                i.saturating_add(1)
            } else {
                i.saturating_sub(1)
            };
            let have = children.get(i).map_or(1, |c| u32::from(c.weight));
            let theirs = children.get(other).map_or(1, |c| u32::from(c.weight));
            // Growing is paid for by the neighbour, and the neighbour keeps at least 1.
            let wanted = i64::from(by);
            let grow = wanted
                .min(i64::from(theirs).saturating_sub(1))
                .max(-(i64::from(have).saturating_sub(1)));
            let new_have = i64::from(have).saturating_add(grow);
            let new_theirs = i64::from(theirs).saturating_sub(grow);
            let clamp = |v: i64| u16::try_from(v.clamp(1, i64::from(MAX_WEIGHT))).unwrap_or(1);
            if let Some(c) = children.get_mut(i) {
                c.weight = clamp(new_have);
            }
            if let Some(c) = children.get_mut(other) {
                c.weight = clamp(new_theirs);
            }
            true
        }
    }
}

/// Scale shares so they add up to [`NORMAL_TOTAL`] (each at least 1), keeping their proportions.
fn normalize(children: &mut [Child]) {
    let total: u32 = children.iter().map(|c| u32::from(c.weight)).sum();
    if total == 0 {
        return;
    }
    let mut scaled: Vec<u32> = children
        .iter()
        .map(|c| (u32::from(c.weight).saturating_mul(NORMAL_TOTAL) / total).max(1))
        .collect();
    // Hand the rounding difference to the largest share so the total is exact again.
    let sum: u32 = scaled.iter().sum();
    if let Some(biggest) = (0..scaled.len()).max_by_key(|&i| scaled.get(i).copied().unwrap_or(0)) {
        if let Some(v) = scaled.get_mut(biggest) {
            *v = (*v).saturating_add(NORMAL_TOTAL).saturating_sub(sum).max(1);
        }
    }
    for (c, w) in children.iter_mut().zip(scaled) {
        c.weight = u16::try_from(w).unwrap_or(1);
    }
}

/// Move the active tab of the nearest tab set holding `focus`; the surface that should then have
/// the keyboard (the first one showing in the new tab).
fn step_in(region: &mut Region, focus: &SurfaceId, forward: bool) -> Option<SurfaceId> {
    match region {
        Region::Surface(_) => None,
        Region::Split { children, .. } => children
            .iter_mut()
            .find(|c| contains(&c.region, focus))
            .and_then(|c| step_in(&mut c.region, focus, forward)),
        Region::Tabs { active, tabs } => {
            // A tab set inside the focused tab is nearer to the focus than this one.
            if let Some(i) = tabs.iter().position(|t| contains(t, focus)) {
                if let Some(t) = tabs.get_mut(i) {
                    if let Some(inner) = step_in(t, focus, forward) {
                        return Some(inner);
                    }
                }
            }
            let n = tabs.len();
            if n < 2 {
                return None;
            }
            *active = if forward {
                active.saturating_add(1) % n
            } else {
                active.checked_sub(1).unwrap_or(n.saturating_sub(1))
            };
            let mut shown = Vec::new();
            tabs.get(*active)?.visible(&mut shown);
            shown.first().map(|s| (*s).clone())
        }
    }
}
