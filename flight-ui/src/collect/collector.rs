// SPDX-License-Identifier: MIT

use super::resolve_pane::resolve_pane;
use crate::snapshot::{HostHealth, HostView, PanePreview, PaneView, SurfaceKind, UiSnapshot};
use flight_classify::{detect_agent, prune_tracking, why, AgentKind, ResolvedState};
use flight_control::{HostError, HostPane, HostRegistry, PanesOutcome};
use flight_state::{HostId, PaneRef, RawPlacement, ServerId, SurfaceRole};
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
            saved: Vec::new(),
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
                    label: o.host.to_string(),
                    host: o.host,
                    server: o.server,
                    health: HostHealth::Online,
                    panes: views,
                }
            }
            Err(e) => HostView {
                label: o.host.to_string(),
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
        let placement = RawPlacement {
            workspace_id: p.info.workspace_id.clone(),
            surface_id: p.info.surface_id.clone(),
            surface_kind: p.info.surface_kind.clone(),
            window_id: p.info.window_id.clone(),
            session_id: p.info.session_id.clone(),
            session_path: p.info.session_path.clone(),
        };
        let view = PaneView {
            pane_ref: p.pane_ref.clone(),
            session: p.info.session_name.clone(),
            window: p.info.window_name.clone(),
            agent,
            state: resolved.state,
            why: why(&resolved),
            title: p.info.pane_title.clone(),
            pid: p.info.pane_pid,
            workspace: placement.workspace(&p.pane_ref.host, &p.pane_ref.server),
            surface: placement.surface(&p.pane_ref.host, &p.pane_ref.server),
            kind: match placement.role(agent != AgentKind::Other) {
                SurfaceRole::Agent => SurfaceKind::Agent(agent),
                SurfaceRole::Shell => SurfaceKind::Shell,
            },
            root: placement.root(&p.info.current_path).to_owned(),
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

impl super::Backend for Collector {
    fn snapshot(&mut self, now: u64) -> UiSnapshot {
        self.collect(now)
    }

    fn preview(&mut self, pane: &PaneRef) -> PanePreview {
        Collector::preview(self, pane)
    }

    fn switch_to(&mut self, pane: &PaneView) -> Result<(), String> {
        Collector::switch_to(self, &pane.pane_ref).map_err(|e| e.to_string())
    }
}
