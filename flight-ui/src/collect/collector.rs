// SPDX-License-Identifier: MIT

use super::resolve_pane::resolve_pane;
use crate::snapshot::{HostHealth, HostView, PanePreview, PaneView, UiSnapshot};
use flight_classify::{detect_agent, prune_tracking, why, ResolvedState};
use flight_control::{HostError, HostPane, HostRegistry, PanesOutcome};
use flight_state::{HostId, PaneRef, ServerId};
use std::collections::{HashMap, HashSet};

/// Lines captured for the preview of the selected pane.
const PREVIEW_LINES: u32 = 200;

/// Turns the registry's raw observations into a [`UiSnapshot`], keeping the per-pane
/// temporal state ([`ResolvedState`]) between refreshes. This is the only place the UI
/// stack performs I/O; one unreachable host never fails the refresh.
pub struct Collector {
    registry: HostRegistry,
    resolved: HashMap<PaneRef, ResolvedState>,
}

impl Collector {
    pub fn new(registry: HostRegistry) -> Self {
        Self {
            registry,
            resolved: HashMap::new(),
        }
    }

    pub fn collect(&mut self, now: u64) -> UiSnapshot {
        let outcomes = self.registry.list_all_panes();
        let hosts: Vec<HostView> = outcomes
            .into_iter()
            .map(|o| self.host_view(o, now))
            .collect();
        self.prune(&hosts);
        UiSnapshot {
            hosts,
            taken_at: now,
        }
    }

    pub fn preview(&self, pane: &PaneRef) -> PanePreview {
        let content = self
            .registry
            .capture_pane(pane, false, Some(PREVIEW_LINES))
            .map(|t| t.lines().map(str::to_owned).collect())
            .map_err(|e: HostError| e.to_string());
        PanePreview {
            pane: pane.clone(),
            content,
        }
    }

    pub fn switch_to(&self, pane: &PaneRef) -> Result<(), HostError> {
        self.registry.switch_to_pane(pane)
    }

    fn host_view(&mut self, o: PanesOutcome, now: u64) -> HostView {
        match o.result {
            Ok(panes) => {
                let mut views: Vec<PaneView> = panes
                    .iter()
                    .filter_map(|p| self.pane_view(p, now))
                    .collect();
                views.sort_by(|a, b| {
                    (&a.session, &a.window, a.pane_ref.pane.as_str()).cmp(&(
                        &b.session,
                        &b.window,
                        b.pane_ref.pane.as_str(),
                    ))
                });
                HostView {
                    host: o.host,
                    server: o.server,
                    health: HostHealth::Online,
                    panes: views,
                }
            }
            Err(e) => HostView {
                host: o.host,
                server: o.server,
                health: HostHealth::from_error(&e),
                panes: Vec::new(),
            },
        }
    }

    fn pane_view(&mut self, p: &HostPane, now: u64) -> Option<PaneView> {
        let agent = detect_agent(&p.info.current_command)?;
        let resolved = resolve_pane(
            &self.registry,
            p,
            agent,
            self.resolved.get(&p.pane_ref),
            now,
        );
        let view = PaneView {
            pane_ref: p.pane_ref.clone(),
            session: p.info.session_name.clone(),
            window: p.info.window_name.clone(),
            agent,
            state: resolved.state,
            why: why(&resolved),
            title: p.info.pane_title.clone(),
        };
        self.resolved.insert(p.pane_ref.clone(), resolved);
        Some(view)
    }

    /// Forget panes that vanished from a host that answered. Panes on a host that did not
    /// answer keep their memory: a transient outage must not lose a pending Done.
    fn prune(&mut self, hosts: &[HostView]) {
        let mut live: HashSet<PaneRef> = hosts
            .iter()
            .flat_map(|h| h.panes.iter().map(|p| p.pane_ref.clone()))
            .collect();
        let answered: HashSet<(&HostId, &ServerId)> = hosts
            .iter()
            .filter(|h| h.health == HostHealth::Online)
            .map(|h| (&h.host, &h.server))
            .collect();
        live.extend(
            self.resolved
                .keys()
                .filter(|k| !answered.contains(&(&k.host, &k.server)))
                .cloned(),
        );
        prune_tracking(&mut self.resolved, &live);
    }
}
