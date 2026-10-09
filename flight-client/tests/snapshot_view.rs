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
        saved: vec![],
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

mod saved {
    use super::*;
    use flight_proto::{
        fleet_change, ui_event_body, FleetDelta, FleetImage, FleetSnapshot, NodeSavedWorkspaces,
        SavedHealthCode, SavedRootCode, SavedWorkspace, UiEvent,
    };
    use flight_ui::{HostHealth, SavedHealth, SavedRoot};

    fn entry(key: &str, root_state: SavedRootCode) -> SavedWorkspace {
        SavedWorkspace {
            config_key: key.to_owned(),
            name: key.to_owned(),
            root: "/work/x".to_owned(),
            health: SavedHealthCode::Blocked as i32,
            root_state: root_state as i32,
            detail: "no such directory".to_owned(),
            workspace_id: String::new(),
            imported: false,
        }
    }

    fn image_with(status: NodeStatusCode, saved: Vec<SavedWorkspace>) -> FleetImage {
        let mut n = node("n1", "dev1", status, AvailabilityCode::Available);
        n.saved = saved;
        let mut image = FleetImage::new();
        image.apply(&UiEvent {
            body: Some(ui_event_body::Body::Snapshot(FleetSnapshot {
                incarnation: vec![1; 16],
                nodes: vec![n],
            })),
        });
        image
    }

    #[test]
    fn a_saved_workspace_is_listed_with_its_host_root_and_failure() {
        let image = image_with(
            NodeStatusCode::Online,
            vec![entry("c-1", SavedRootCode::Missing)],
        );
        let snap = flight_client::ui_snapshot(&image, true, None, 5);
        assert_eq!(snap.saved.len(), 1);
        let s = &snap.saved[0];
        assert_eq!(
            (s.host_label.as_str(), s.root.as_str()),
            ("dev1", "/work/x")
        );
        assert_eq!(
            (s.health, s.root_state),
            (SavedHealth::Blocked, SavedRoot::Missing)
        );
        assert_eq!(
            (s.detail.as_str(), s.host_health.clone()),
            ("no such directory", HostHealth::Online)
        );
    }

    #[test]
    fn a_saved_workspace_stays_listed_when_its_node_is_gone_or_the_orchestrator_is() {
        let image = image_with(
            NodeStatusCode::Disconnected,
            vec![entry("c-1", SavedRootCode::Verified)],
        );
        let gone = flight_client::ui_snapshot(&image, true, None, 5);
        assert_eq!(gone.saved.len(), 1);
        assert_eq!(gone.saved[0].host_health, HostHealth::Disconnected);
        let offline = flight_client::ui_snapshot(&image, false, Some("down"), 5);
        assert_eq!(offline.saved.len(), 1);
        assert_eq!(offline.saved[0].host_health, HostHealth::Stale);
    }

    #[test]
    fn a_delta_replaces_the_list() {
        let mut image = image_with(
            NodeStatusCode::Online,
            vec![entry("c-1", SavedRootCode::Missing)],
        );
        image.apply(&UiEvent {
            body: Some(ui_event_body::Body::Delta(FleetDelta {
                incarnation: vec![1; 16],
                sequence: 1,
                change: Some(fleet_change::Change::NodeSaved(NodeSavedWorkspaces {
                    node_id: "n1".to_owned(),
                    items: vec![
                        entry("c-2", SavedRootCode::Changed),
                        entry("c-3", SavedRootCode::Verified),
                    ],
                })),
            })),
        });
        let keys: Vec<_> = flight_client::ui_snapshot(&image, true, None, 5)
            .saved
            .into_iter()
            .map(|s| s.config_key)
            .collect();
        assert_eq!(keys, vec!["c-2", "c-3"]);
    }

    #[test]
    fn an_entry_with_a_code_this_build_does_not_know_is_skipped_not_guessed() {
        let mut odd = entry("c-9", SavedRootCode::Missing);
        odd.health = 99;
        let image = image_with(
            NodeStatusCode::Online,
            vec![odd, entry("c-1", SavedRootCode::Missing)],
        );
        assert_eq!(
            flight_client::ui_snapshot(&image, true, None, 5)
                .saved
                .len(),
            1
        );
    }
}
