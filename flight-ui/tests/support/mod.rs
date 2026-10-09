// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use flight_classify::AgentKind;
use flight_state::{AgentState, HostId, PaneId, PaneRef, ServerId, SurfaceId, WorkspaceId};
use flight_ui::{HostHealth, HostView, PaneView, SurfaceKind, UiSnapshot};

pub fn pref(host: &str, id: &str) -> PaneRef {
    PaneRef {
        host: HostId::new(host),
        server: ServerId::new("flight"),
        pane: PaneId::new(id),
    }
}

pub fn pane(host: &str, session: &str, id: &str, state: AgentState) -> PaneView {
    PaneView {
        pane_ref: pref(host, id),
        session: session.to_owned(),
        window: "w".to_owned(),
        agent: AgentKind::Claude,
        state,
        why: "test".to_owned(),
        title: String::new(),
        pid: 1,
        workspace: workspace_of(host, session),
        surface: SurfaceId::new(format!("s-{id}")),
        kind: SurfaceKind::Agent(AgentKind::Claude),
        root: "/work".to_owned(),
    }
}

/// The workspace a test's agent pane belongs to: named by host and session, like a real one is
/// by the node's id (never by the pane).
pub fn workspace_of(host: &str, session: &str) -> WorkspaceId {
    WorkspaceId::new(format!("w-{host}-{session}"))
}

/// A shell pane in the same workspace as `pane(host, session, ...)`.
pub fn shell_pane(host: &str, session: &str, id: &str) -> PaneView {
    PaneView {
        agent: AgentKind::Other,
        state: AgentState::Shell,
        kind: SurfaceKind::Shell,
        ..pane(host, session, id, AgentState::Shell)
    }
}

pub fn online(host: &str, panes: Vec<PaneView>) -> HostView {
    HostView {
        host: HostId::new(host),
        label: host.to_owned(),
        server: ServerId::new("flight"),
        health: HostHealth::Online,
        panes,
    }
}

pub fn down(host: &str, health: HostHealth) -> HostView {
    HostView {
        host: HostId::new(host),
        label: host.to_owned(),
        server: ServerId::new("flight"),
        health,
        panes: Vec::new(),
    }
}

pub fn snap(hosts: Vec<HostView>) -> UiSnapshot {
    UiSnapshot {
        hosts,
        taken_at: 1000,
    }
}
