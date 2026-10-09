// SPDX-License-Identifier: MIT

//! The one selectable list: every workspace, most urgent first, narrowed by the search text.

use crate::snapshot::{HostView, PaneView, Surface, SurfaceKind, UiSnapshot, Workspace};
use flight_state::{sort_rank, WorkspaceId};
use std::collections::BTreeMap;

/// Workspaces matching `filter`, most urgent first (by their agent's state); ties broken by
/// host (in the order the snapshot lists them), then name, then identity, so the order
/// depends only on state and names.
pub fn workspaces(s: &UiSnapshot, filter: &str) -> Vec<Workspace> {
    let mut all: Vec<(usize, Workspace)> = Vec::new();
    for (index, host) in s.hosts.iter().enumerate() {
        all.extend(host_workspaces(host).into_iter().map(|w| (index, w)));
    }
    all.retain(|(_, w)| matches_filter(w, filter));
    all.sort_by(|(ia, a), (ib, b)| {
        (sort_rank(a.state()), ia, &a.name, &a.id).cmp(&(sort_rank(b.state()), ib, &b.name, &b.id))
    });
    all.into_iter().map(|(_, w)| w).collect()
}

/// The workspaces of one host, with the surfaces of each (an agent first, then a shell).
fn host_workspaces(host: &HostView) -> Vec<Workspace> {
    let mut by_id: BTreeMap<&WorkspaceId, Vec<&PaneView>> = BTreeMap::new();
    for pane in &host.panes {
        by_id.entry(&pane.workspace).or_default().push(pane);
    }
    by_id
        .into_iter()
        .filter_map(|(id, panes)| {
            let first = panes.first()?;
            let mut surfaces: Vec<Surface> = panes.iter().map(|p| Surface::from(*p)).collect();
            surfaces.sort_by_key(|s| (!s.kind.is_agent(), s.id.clone()));
            Some(Workspace {
                id: id.clone(),
                name: first.session.clone(),
                host: host.host.clone(),
                host_label: host.label.clone(),
                root: first.root.clone(),
                surfaces,
            })
        })
        .collect()
}

/// Case-insensitive substring of the workspace name, the host's display name, or what its
/// surfaces are (`claude`, `shell`).
fn matches_filter(w: &Workspace, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || w.name.to_lowercase().contains(&needle)
        || w.host_label.to_lowercase().contains(&needle)
        || w.surfaces.iter().any(|s| {
            match s.kind {
                SurfaceKind::Agent(a) => format!("{a:?}"),
                SurfaceKind::Shell => "shell".to_owned(),
            }
            .to_lowercase()
            .contains(&needle)
        })
}
