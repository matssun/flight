// SPDX-License-Identifier: MIT

//! The one selectable list: every session, most urgent first, narrowed by the search text.

use crate::snapshot::{HostView, PaneView, UiSnapshot};
use flight_state::sort_rank;

/// Sessions matching `filter`, most urgent first; ties broken by host (in the order the
/// snapshot lists them), then name, then identity, so the order depends only on state and names.
pub fn sessions<'a>(s: &'a UiSnapshot, filter: &str) -> Vec<&'a PaneView> {
    let mut v: Vec<(usize, &HostView, &PaneView)> = s
        .hosts
        .iter()
        .enumerate()
        .flat_map(|(i, h)| h.panes.iter().map(move |p| (i, h, p)))
        .filter(|(_, h, p)| matches_filter(h, p, filter))
        .collect();
    v.sort_by(|(ia, _, a), (ib, _, b)| {
        (sort_rank(a.state), ia, &a.session, &a.pane_ref).cmp(&(
            sort_rank(b.state),
            ib,
            &b.session,
            &b.pane_ref,
        ))
    });
    v.into_iter().map(|(_, _, p)| p).collect()
}

/// Case-insensitive substring of the session name, the host's display name or the agent kind.
fn matches_filter(host: &HostView, pane: &PaneView, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || pane.session.to_lowercase().contains(&needle)
        || host.label.to_lowercase().contains(&needle)
        || format!("{:?}", pane.agent).to_lowercase().contains(&needle)
}
