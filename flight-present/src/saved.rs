// SPDX-License-Identifier: MIT

//! A layout as it is kept: versioned text a person could read, read back with suspicion.
//!
//! A saved layout names surfaces by id and nothing else. Reading one checks it like any input
//! (bounded size, valid ids, the layout's own invariants) and then fits it to the surfaces that
//! exist now: a surface that is gone is dropped from the tree, which closes up around it, and a
//! layout with nothing left is no layout. A surface that is not named gets no place; it is not
//! forced in.

use crate::layout::{MAX_DEPTH, MAX_SURFACES};
use crate::{Axis, Child, Layout, LayoutError, Region};
use flight_state::{valid_id, SurfaceId};
use serde::{Deserialize, Serialize};

/// The schema this build writes and reads.
pub const VERSION: u32 = 1;
/// The most text a saved layout may be: far more than any layout this build accepts needs.
pub const MAX_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
struct Doc {
    version: u32,
    focus: String,
    root: Node,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Node {
    Surface {
        id: String,
    },
    Split {
        axis: AxisName,
        children: Vec<ChildNode>,
    },
    Tabs {
        active: usize,
        tabs: Vec<Node>,
    },
}

#[derive(Serialize, Deserialize)]
struct ChildNode {
    weight: u16,
    region: Node,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum AxisName {
    Across,
    Down,
}

/// The saved text of `layout`.
pub fn to_toml(layout: &Layout) -> Result<String, LayoutError> {
    let doc = Doc {
        version: VERSION,
        focus: layout.focus().to_string(),
        root: node_of(layout.root()),
    };
    toml::to_string_pretty(&doc).map_err(|e| LayoutError::Unreadable(e.to_string()))
}

/// The layout in `text`, fitted to the surfaces for which `exists` is true. Errors for text that
/// is malformed, newer than this build, too big, or invalid; `Err(Empty)` when none of its
/// surfaces exist any more.
pub fn from_toml(text: &str, exists: impl Fn(&SurfaceId) -> bool) -> Result<Layout, LayoutError> {
    if text.len() > MAX_BYTES {
        return Err(LayoutError::Unreadable("too big".to_owned()));
    }
    let doc: Doc = toml::from_str(text).map_err(|e| LayoutError::Unreadable(e.to_string()))?;
    if doc.version != VERSION {
        return Err(LayoutError::Unreadable(format!(
            "schema {} (this build reads {VERSION})",
            doc.version
        )));
    }
    let root = region_of(&doc.root, 1)?;
    let mut shown = Vec::new();
    root.visible(&mut shown);
    // A focus that is not showing (edited by hand, or its tab changed) is not an error: the
    // first showing surface has the keyboard.
    let focus = SurfaceId::new(&doc.focus);
    let focus = if valid_id(&doc.focus) && shown.contains(&&focus) {
        focus
    } else {
        shown
            .first()
            .map(|s| (*s).clone())
            .ok_or(LayoutError::Empty)?
    };
    Layout::new(root, focus)?
        .retain(exists)
        .ok_or(LayoutError::Empty)
}

fn node_of(region: &Region) -> Node {
    match region {
        Region::Surface(s) => Node::Surface { id: s.to_string() },
        Region::Split { axis, children } => Node::Split {
            axis: match axis {
                Axis::Across => AxisName::Across,
                Axis::Down => AxisName::Down,
            },
            children: children
                .iter()
                .map(|c| ChildNode {
                    weight: c.weight,
                    region: node_of(&c.region),
                })
                .collect(),
        },
        Region::Tabs { active, tabs } => Node::Tabs {
            active: *active,
            tabs: tabs.iter().map(node_of).collect(),
        },
    }
}

/// Read the tree, refusing what is too deep or too wide as it goes (before building it all).
fn region_of(node: &Node, depth: usize) -> Result<Region, LayoutError> {
    if depth > MAX_DEPTH {
        return Err(LayoutError::TooDeep);
    }
    match node {
        Node::Surface { id } => {
            if !valid_id(id) {
                return Err(LayoutError::Unreadable(
                    "a surface id is not plain text".to_owned(),
                ));
            }
            Ok(Region::Surface(SurfaceId::new(id)))
        }
        Node::Split { axis, children } => {
            if children.len() > MAX_SURFACES {
                return Err(LayoutError::TooMany);
            }
            Ok(Region::Split {
                axis: match axis {
                    AxisName::Across => Axis::Across,
                    AxisName::Down => Axis::Down,
                },
                children: children
                    .iter()
                    .map(|c| {
                        Ok(Child::new(
                            c.weight,
                            region_of(&c.region, depth.saturating_add(1))?,
                        ))
                    })
                    .collect::<Result<_, LayoutError>>()?,
            })
        }
        Node::Tabs { active, tabs } => {
            if tabs.len() > MAX_SURFACES {
                return Err(LayoutError::TooMany);
            }
            Ok(Region::Tabs {
                active: *active,
                tabs: tabs
                    .iter()
                    .map(|t| region_of(t, depth.saturating_add(1)))
                    .collect::<Result<_, LayoutError>>()?,
            })
        }
    }
}
