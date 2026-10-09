// SPDX-License-Identifier: MIT

use crate::{LayoutError, Region};
use flight_state::SurfaceId;

/// How deeply regions may nest: a layout is for a person to look at, not for a program to build.
pub const MAX_DEPTH: usize = 6;
/// The most surfaces one layout names.
pub const MAX_SURFACES: usize = 16;
/// The largest share a child may be given.
pub const MAX_WEIGHT: u16 = 10_000;

/// A tree of regions and the surface that has the keyboard.
///
/// Invariants, checked by [`Layout::new`] and kept by every edit: at least one surface; no
/// surface twice; every split and tab set non-empty; shares between 1 and [`MAX_WEIGHT`]; the
/// active tab a real tab; the focus a surface that is showing. Nothing here says what a surface
/// is or how long it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub(crate) root: Region,
    pub(crate) focus: SurfaceId,
}

impl Layout {
    /// One surface, filling the terminal: the focused, single-surface presentation.
    pub fn single(surface: SurfaceId) -> Self {
        Self {
            root: Region::Surface(surface.clone()),
            focus: surface,
        }
    }

    /// A layout, if it satisfies the invariants.
    pub fn new(root: Region, focus: SurfaceId) -> Result<Self, LayoutError> {
        let layout = Self { root, focus };
        layout.check()?;
        Ok(layout)
    }

    pub fn root(&self) -> &Region {
        &self.root
    }

    /// The surface that receives the keyboard.
    pub fn focus(&self) -> &SurfaceId {
        &self.focus
    }

    /// Every surface the layout names, showing or not, in reading order.
    pub fn surfaces(&self) -> Vec<&SurfaceId> {
        let mut out = Vec::new();
        self.root.surfaces(&mut out);
        out
    }

    /// The surfaces that are on screen (not in an inactive tab), in reading order. Only these
    /// need an attachment.
    pub fn visible(&self) -> Vec<&SurfaceId> {
        let mut out = Vec::new();
        self.root.visible(&mut out);
        out
    }

    pub fn contains(&self, surface: &SurfaceId) -> bool {
        self.surfaces().contains(&surface)
    }

    pub(crate) fn check(&self) -> Result<(), LayoutError> {
        if self.root.depth() > MAX_DEPTH {
            return Err(LayoutError::TooDeep);
        }
        check_region(&self.root)?;
        let all = self.surfaces();
        if all.is_empty() {
            return Err(LayoutError::Empty);
        }
        if all.len() > MAX_SURFACES {
            return Err(LayoutError::TooMany);
        }
        for (i, s) in all.iter().enumerate() {
            if all.iter().skip(i.saturating_add(1)).any(|t| t == s) {
                return Err(LayoutError::Duplicate(s.to_string()));
            }
        }
        if !self.visible().contains(&&self.focus) {
            return Err(LayoutError::FocusNotShowing);
        }
        Ok(())
    }
}

fn check_region(region: &Region) -> Result<(), LayoutError> {
    match region {
        Region::Surface(_) => Ok(()),
        Region::Split { children, .. } => {
            if children.is_empty() {
                return Err(LayoutError::EmptyRegion);
            }
            for c in children {
                if c.weight == 0 || c.weight > MAX_WEIGHT {
                    return Err(LayoutError::BadWeight);
                }
                check_region(&c.region)?;
            }
            Ok(())
        }
        Region::Tabs { active, tabs } => {
            if tabs.is_empty() {
                return Err(LayoutError::EmptyRegion);
            }
            if *active >= tabs.len() {
                return Err(LayoutError::BadTab);
            }
            tabs.iter().try_for_each(check_region)
        }
    }
}
