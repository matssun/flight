// SPDX-License-Identifier: MIT

//! How a fleet image becomes dashboard data: liveness and availability map to host health,
//! last-known panes stay visible, and an unreachable orchestrator is explicit.

use flight_client::ui_snapshot;
use flight_proto::{
    ui_event_body, AgentKindCode, AvailabilityCode, FleetImage, FleetSnapshot, Incarnation,
    NodeStatusCode, NodeView, PaneRefMsg, PaneState, ServerStatus, SourceCode, StateCode, UiEvent,
};
use flight_state::AgentState;
use flight_ui::HostHealth;

fn pane(node: &str, id: &str, state: StateCode) -> PaneState {
    PaneState {
        pane_ref: Some(PaneRefMsg {
            host: node.into(),
            server: "flight".into(),
            pane: id.into(),
        }),
        agent_kind: AgentKindCode::Codex as i32,
        state: state as i32,
        source: SourceCode::Scrape as i32,
        rule_id: String::new(),
        why: "because".into(),
        changed_at: 1,
        session: "work".into(),
        window: "w".into(),
        path: String::new(),
        command: "codex".into(),
        pid: 0,
        ..Default::default()
    }
}

fn server(availability: AvailabilityCode, detail: &str) -> ServerStatus {
    ServerStatus {
        server: "flight".into(),
        availability: availability as i32,
        detail: detail.into(),
    }
}

fn node(id: &str, name: &str, status: NodeStatusCode, availability: AvailabilityCode) -> NodeView {
    NodeView {
        node_id: id.into(),
        display_name: name.into(),
        status: status as i32,
        servers: vec![server(availability, "boom")],
        panes: vec![pane(id, "%1", StateCode::Permit)],
    }
}

fn image(nodes: Vec<NodeView>) -> FleetImage {
    let mut image = FleetImage::new();
    image.apply(&UiEvent {
        body: Some(ui_event_body::Body::Snapshot(FleetSnapshot {
            incarnation: Incarnation::from_bytes([1; Incarnation::LEN])
                .as_bytes()
                .to_vec(),
            nodes,
        })),
    });
    image
}

#[test]
fn liveness_and_availability_become_host_health() {
    let img = image(vec![
        node(
            "n-a",
            "alpha",
            NodeStatusCode::Online,
            AvailabilityCode::Available,
        ),
        node(
            "n-b",
            "bravo",
            NodeStatusCode::Stale,
            AvailabilityCode::Available,
        ),
        node(
            "n-c",
            "charlie",
            NodeStatusCode::Disconnected,
            AvailabilityCode::Available,
        ),
        node(
            "n-d",
            "delta",
            NodeStatusCode::Online,
            AvailabilityCode::NoServer,
        ),
        node(
            "n-e",
            "echo",
            NodeStatusCode::Online,
            AvailabilityCode::TmuxUnavailable,
        ),
        node(
            "n-f",
            "foxtrot",
            NodeStatusCode::Online,
            AvailabilityCode::Failed,
        ),
    ]);
    let s = ui_snapshot(&img, true, None, 5);
    let health: Vec<_> = s.hosts.iter().map(|h| h.health.clone()).collect();
    assert_eq!(
        health,
        vec![
            HostHealth::Online,
            HostHealth::Stale,
            HostHealth::Disconnected,
            HostHealth::NoServer,
            HostHealth::NoTmux,
            HostHealth::Failed("boom".into()),
        ]
    );
}

#[test]
fn last_known_panes_stay_visible_when_a_node_is_away_and_keep_their_state() {
    let img = image(vec![node(
        "n-c",
        "charlie",
        NodeStatusCode::Disconnected,
        AvailabilityCode::Available,
    )]);
    let s = ui_snapshot(&img, true, None, 5);
    assert_eq!(s.hosts[0].panes.len(), 1);
    assert_eq!(
        s.hosts[0].panes[0].state,
        AgentState::Permit,
        "liveness never rewrites pane state"
    );
}

#[test]
fn a_dead_orchestrator_link_is_an_explicit_row_and_everything_else_is_stale() {
    let img = image(vec![node(
        "n-a",
        "alpha",
        NodeStatusCode::Online,
        AvailabilityCode::Available,
    )]);
    let s = ui_snapshot(&img, false, Some("connection refused"), 5);
    assert_eq!(s.hosts[0].label, "orchestrator");
    assert_eq!(
        s.hosts[0].health,
        HostHealth::Unreachable("connection refused".into())
    );
    assert_eq!(s.hosts[1].health, HostHealth::Stale);
    assert_eq!(s.hosts[1].panes.len(), 1);
}

#[test]
fn nodes_are_listed_by_display_name_and_identified_by_id() {
    let img = image(vec![
        node(
            "n-z",
            "alpha",
            NodeStatusCode::Online,
            AvailabilityCode::Available,
        ),
        node(
            "n-a",
            "zulu",
            NodeStatusCode::Online,
            AvailabilityCode::Available,
        ),
    ]);
    let s = ui_snapshot(&img, true, None, 5);
    let labels: Vec<_> = s
        .hosts
        .iter()
        .map(|h| (h.label.as_str(), h.host.as_str()))
        .collect();
    assert_eq!(labels, vec![("alpha", "n-z"), ("zulu", "n-a")]);
    assert_eq!(
        s.hosts[0].panes[0].pane_ref.host.as_str(),
        "n-z",
        "pane identity is the node id"
    );
}

#[test]
fn a_node_with_no_panes_yet_is_still_listed() {
    let mut v = node(
        "n-a",
        "alpha",
        NodeStatusCode::Online,
        AvailabilityCode::Available,
    );
    v.panes.clear();
    v.servers.clear();
    let s = ui_snapshot(&image(vec![v]), true, None, 5);
    assert_eq!(s.hosts.len(), 1);
    assert_eq!(s.hosts[0].health, HostHealth::Online);
}
