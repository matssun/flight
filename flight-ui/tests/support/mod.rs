// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use flight_classify::AgentKind;
use flight_state::{AgentState, HostId, PaneId, PaneRef, ServerId};
use flight_ui::{HostHealth, HostView, PaneView, UiSnapshot};

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
