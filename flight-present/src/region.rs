// SPDX-License-Identifier: MIT

use crate::{Axis, Child};
use flight_state::SurfaceId;

/// A part of a layout: one surface, a split into regions, or tabs of which one shows.
///
/// A region refers to surfaces by identity only. The surface lives wherever it lives (ADR-007);
/// a region that stops showing it does not end it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    Surface(SurfaceId),
    Split {
        axis: Axis,
        children: Vec<Child>,
    },
    /// Alternatives in the same space. Only the active one is visible; the others hold no
    /// attachment (they cost nothing until shown).
    Tabs {
        active: usize,
        tabs: Vec<Region>,
    },
}

impl Region {
    /// Every surface named under this region, in reading order (left to right, top to bottom,
    /// tabs in order), including the ones in tabs that are not showing.
    pub fn surfaces<'a>(&'a self, out: &mut Vec<&'a SurfaceId>) {
        match self {
            Self::Surface(s) => out.push(s),
            Self::Split { children, .. } => {
                for c in children {
                    c.region.surfaces(out);
                }
            }
            Self::Tabs { tabs, .. } => {
                for t in tabs {
                    t.surfaces(out);
                }
            }
        }
    }

    /// The surfaces that are showing: like [`Self::surfaces`] but only the active tab.
    pub fn visible<'a>(&'a self, out: &mut Vec<&'a SurfaceId>) {
        match self {
            Self::Surface(s) => out.push(s),
            Self::Split { children, .. } => {
                for c in children {
                    c.region.visible(out);
                }
            }
            Self::Tabs { active, tabs } => {
                if let Some(t) = tabs.get(*active) {
                    t.visible(out);
                }
            }
        }
    }

    /// How deeply regions are nested under this one (a lone surface is depth 1).
    pub fn depth(&self) -> usize {
        match self {
            Self::Surface(_) => 1,
            Self::Split { children, .. } => {
                1usize.saturating_add(children.iter().map(|c| c.region.depth()).max().unwrap_or(0))
            }
            Self::Tabs { tabs, .. } => {
                1usize.saturating_add(tabs.iter().map(Region::depth).max().unwrap_or(0))
            }
        }
    }
}
