// SPDX-License-Identifier: MIT

//! Pure projections of a snapshot into the two selectable lists. The attention list is a
//! projection of the same pane records the tree shows, never a second copy of state.

use super::Section;
use crate::snapshot::{PaneView, UiSnapshot};
use flight_state::{needs_attention, sort_rank};

/// Panes that need the user, most urgent first, ties broken stably by identity.
pub fn attention_panes(s: &UiSnapshot) -> Vec<&PaneView> {
    let mut v: Vec<&PaneView> = tree_panes(s)
        .into_iter()
        .filter(|p| needs_attention(p.state))
        .collect();
    v.sort_by_key(|p| (sort_rank(p.state), p.pane_ref.clone()));
    v
}

/// Every agent pane in the stable tree order: host, then session, window, pane. The order
/// does not depend on state, so states changing never reorder the tree.
pub fn tree_panes(s: &UiSnapshot) -> Vec<&PaneView> {
    s.hosts.iter().flat_map(|h| h.panes.iter()).collect()
}

pub fn section_panes(s: &UiSnapshot, section: Section) -> Vec<&PaneView> {
    match section {
        Section::Attention => attention_panes(s),
        Section::Tree => tree_panes(s),
    }
}
