// SPDX-License-Identifier: MIT

use flight_classify::AgentKind;
use flight_proto::{
    AgentKindCode, AvailabilityCode, FleetImage, FleetNode, NodeStatusCode, PaneState,
    SavedHealthCode, SavedResumeCode, SavedRootCode, SavedWorkspace, SurfaceKindCode,
};
use flight_state::{HostId, PaneRef, ServerId, SurfaceId, WorkspaceId};
use flight_ui::{
    HostHealth, HostView, PaneView, SavedHealth, SavedResume, SavedRoot, SavedView, SurfaceKind,
    UiSnapshot,
};
use std::collections::BTreeSet;

/// The orchestrator link itself, shown as a host row when it is down.
const LINK_HOST: &str = "orchestrator";

/// What the dashboard shows for a fleet image. A pure function: `connected` says whether the
/// stream to the orchestrator is up right now. While it is not, the image is last-known and
/// every node is shown as stale beneath an explicit "orchestrator unreachable" row.
pub fn ui_snapshot(
    image: &FleetImage,
    connected: bool,
    link_error: Option<&str>,
    now: u64,
) -> UiSnapshot {
    let mut hosts = Vec::new();
    if !connected {
        hosts.push(HostView {
            host: HostId::new(LINK_HOST),
            label: LINK_HOST.to_owned(),
            server: ServerId::new("-"),
            health: HostHealth::Unreachable(link_error.unwrap_or("not connected").to_owned()),
            panes: Vec::new(),
        });
    }
    let mut nodes: Vec<(&String, &FleetNode)> = image.nodes().iter().collect();
    nodes.sort_by(|a, b| (&a.1.display_name, a.0).cmp(&(&b.1.display_name, b.0)));
    let mut saved = Vec::new();
    for (id, node) in nodes {
        hosts.extend(node_hosts(id, node, connected));
        saved.extend(saved_views(id, node, connected));
    }
    UiSnapshot {
        hosts,
        saved,
        taken_at: now,
    }
}

/// The node's saved workspaces, each with how reachable its node is. A node that is stale or
/// gone still lists them (last-known): a saved workspace never vanishes with its host.
fn saved_views(id: &str, node: &FleetNode, connected: bool) -> Vec<SavedView> {
    let host_health = match (connected, node.status_code()) {
        (true, Some(NodeStatusCode::Online)) => HostHealth::Online,
        (true, Some(NodeStatusCode::Disconnected) | None) => HostHealth::Disconnected,
        _ => HostHealth::Stale,
    };
    node.saved
        .iter()
        .filter_map(|s| saved_view(id, node, &host_health, s))
        .collect()
}

fn saved_view(
    id: &str,
    node: &FleetNode,
    host_health: &HostHealth,
    s: &SavedWorkspace,
) -> Option<SavedView> {
    let health = match SavedHealthCode::try_from(s.health).ok()? {
        SavedHealthCode::Running => SavedHealth::Running,
        SavedHealthCode::Partial => SavedHealth::Partial,
        SavedHealthCode::Stopped => SavedHealth::Stopped,
        SavedHealthCode::Blocked | SavedHealthCode::Unspecified => SavedHealth::Blocked,
    };
    let root_state = match SavedRootCode::try_from(s.root_state).ok()? {
        SavedRootCode::Verified => SavedRoot::Verified,
        SavedRootCode::FirstSighting => SavedRoot::FirstSighting,
        SavedRootCode::Missing => SavedRoot::Missing,
        SavedRootCode::NotADirectory => SavedRoot::NotADirectory,
        SavedRootCode::PermissionDenied => SavedRoot::PermissionDenied,
        SavedRootCode::Changed => SavedRoot::Changed,
        SavedRootCode::Unverified | SavedRootCode::Unspecified => SavedRoot::Unverified,
    };
    Some(SavedView {
        host: HostId::new(id),
        host_label: node.display_name.clone(),
        host_health: host_health.clone(),
        config_key: s.config_key.clone(),
        name: s.name.clone(),
        root: s.root.clone(),
        health,
        root_state,
        detail: s.detail.clone(),
        running: (!s.workspace_id.is_empty()).then(|| s.workspace_id.clone()),
        imported: s.imported,
        resume: match SavedResumeCode::try_from(s.resume) {
            Ok(SavedResumeCode::None) => SavedResume::New,
            Ok(SavedResumeCode::Available) => SavedResume::Continues,
            Ok(SavedResumeCode::Unavailable) => {
                SavedResume::CannotContinue(s.resume_detail.clone())
            }
            Ok(SavedResumeCode::Unsupported) => SavedResume::Unsupported(s.resume_detail.clone()),
            Ok(SavedResumeCode::Unspecified) | Err(_) => SavedResume::Unknown,
        },
    })
}

fn node_hosts(id: &str, node: &FleetNode, connected: bool) -> Vec<HostView> {
    let liveness = if connected {
        node.status_code()
    } else {
        Some(NodeStatusCode::Stale)
    };
    let mut servers: BTreeSet<String> = node.servers.keys().cloned().collect();
    servers.extend(node.panes.keys().map(|k| k.server.to_string()));
    if servers.is_empty() {
        servers.insert("flight".to_owned());
    }
    servers
        .into_iter()
        .map(|server| {
            let health = match liveness {
                Some(NodeStatusCode::Online) => availability(node, &server),
                Some(NodeStatusCode::Stale) => HostHealth::Stale,
                _ => HostHealth::Disconnected,
            };
            let mut panes: Vec<PaneView> = node
                .panes
                .iter()
                .filter(|(k, _)| k.server.as_str() == server)
                .filter_map(|(k, p)| pane_view(k, p))
                .collect();
            panes.sort_by(|a, b| {
                (&a.session, &a.window, a.pane_ref.pane.as_str()).cmp(&(
                    &b.session,
                    &b.window,
                    b.pane_ref.pane.as_str(),
                ))
            });
            HostView {
                host: HostId::new(id),
                label: node.display_name.clone(),
                server: ServerId::new(server),
                health,
                panes,
            }
        })
        .collect()
}

fn availability(node: &FleetNode, server: &str) -> HostHealth {
    let Some(status) = node.servers.get(server) else {
        return HostHealth::Online;
    };
    match AvailabilityCode::try_from(status.availability) {
        Ok(AvailabilityCode::Available) => HostHealth::Online,
        Ok(AvailabilityCode::NoServer) => HostHealth::NoServer,
        Ok(AvailabilityCode::TmuxUnavailable) => HostHealth::NoTmux,
        _ => HostHealth::Failed(status.detail.clone()),
    }
}

fn pane_view(key: &PaneRef, p: &PaneState) -> Option<PaneView> {
    let agent = agent_kind(p.agent_kind);
    // A node that predates workspaces publishes none: the pane is then a workspace of its own.
    let workspace = if p.workspace_id.is_empty() {
        WorkspaceId::for_unmarked_session(key.host.as_str(), key.server.as_str(), &p.session)
    } else {
        WorkspaceId::new(&p.workspace_id)
    };
    let surface = if p.surface_id.is_empty() {
        SurfaceId::new(format!(
            "{workspace}.{}",
            key.pane.as_str().trim_start_matches('%')
        ))
    } else {
        SurfaceId::new(&p.surface_id)
    };
    let kind = match SurfaceKindCode::try_from(p.surface_kind) {
        Ok(SurfaceKindCode::Shell) => SurfaceKind::Shell,
        _ => SurfaceKind::Agent(agent),
    };
    Some(PaneView {
        pane_ref: key.clone(),
        session: p.session.clone(),
        window: p.window.clone(),
        agent,
        state: p.agent_state().ok()?,
        why: p.why.clone(),
        // The title changes on every poll and is not replicated.
        title: String::new(),
        pid: p.pid,
        workspace,
        surface,
        kind,
        root: if p.workspace_root.is_empty() {
            p.path.clone()
        } else {
            p.workspace_root.clone()
        },
    })
}

fn agent_kind(code: i32) -> AgentKind {
    match AgentKindCode::try_from(code) {
        Ok(AgentKindCode::Claude) => AgentKind::Claude,
        Ok(AgentKindCode::Codex) => AgentKind::Codex,
        Ok(AgentKindCode::OpenCode) => AgentKind::OpenCode,
        Ok(AgentKindCode::Pi) => AgentKind::Pi,
        _ => AgentKind::Other,
    }
}
