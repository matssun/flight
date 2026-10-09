// SPDX-License-Identifier: MIT

//! The complete localhost cycle over real mutual TLS and tonic:
//! node -> orchestrator -> UI, with hello, snapshot, deltas, preview, disconnect, reconnect
//! and resync.

mod support;

use flight_proto::{
    command_kind as ck, fleet_change::Change, response_result, ui_event_body, ui_request_body,
    Command, NodeStatusCode, Request, Response, StateCode, Subscribe, UiRequest,
};
use flight_transport::UiClient;
use flight_trust::Identity;
use std::sync::Arc;
use support::*;
use tokio::sync::watch;

fn subscribe() -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
    }
}

fn preview(node: &str, pane: &str, id: u64) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::GetPreview(ck::GetPreview {
                    pane_ref: Some(flight_proto::PaneRefMsg {
                        host: node.into(),
                        server: "flight".into(),
                        pane: pane.into(),
                    }),
                    lines: 25,
                })),
            }),
        })),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn node_to_orchestrator_to_ui_over_mutual_tls() {
    let node_id = Arc::new(Identity::generate().expect("node"));
    let ui_id = Identity::generate().expect("ui");
    let trust = trust_with(&[(&node_id, "mini-1")], &[(&ui_id, "laptop")]);
    let (server, orch, addr) = start(trust, None).await;
    let node_fp = node_id.fingerprint().clone();

    // The node dials in; its observations replicate as snapshot then deltas.
    let link = node_link(&node_id, &addr, &orch, "mini-1", None, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("node online", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    link.observe(vec![round(1, vec![obs("%1", 7, PERMIT_SCREEN)])]);
    wait_until("pane replicated", || pane_count(&server, &node_fp) == 1).await;

    // A UI subscribes and receives the whole fleet as one snapshot.
    let mut ui = UiClient::connect(&addr, &ui_id, &orch)
        .await
        .expect("ui connects");
    ui.send(subscribe()).expect("send");
    let first = ui_until(&mut ui, "fleet snapshot", |e| {
        matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
    })
    .await;
    let Some(ui_event_body::Body::Snapshot(snapshot)) = first.body else {
        unreachable!()
    };
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.nodes[0].node_id, node_fp.as_str());
    assert_eq!(snapshot.nodes[0].panes[0].state, StateCode::Permit as i32);

    // A change on the node reaches the UI as a delta.
    link.observe(vec![round(2, vec![obs("%1", 7, BUSY_SCREEN)])]);
    ui_until(&mut ui, "busy delta", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d)) if matches!(&d.change,
            Some(Change::PaneUpsert(p)) if p.state == StateCode::Busy as i32))
    })
    .await;

    // On-demand preview is routed to the node and answered.
    ui.send(preview(node_fp.as_str(), "%1", 42)).expect("send");
    let answer = ui_until(&mut ui, "preview response", |e| {
        matches!(e.body, Some(ui_event_body::Body::Response(_)))
    })
    .await;
    let Some(ui_event_body::Body::Response(Response { request_id, result })) = answer.body else {
        unreachable!()
    };
    assert_eq!(request_id, 42);
    assert!(
        matches!(result, Some(response_result::Result::Preview(p)) if p.text == "preview of %1 (25 lines)")
    );

    // The node goes away: the UI is told, the last-known pane stays, control fails at once.
    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    ui_until(&mut ui, "disconnected status", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d)) if matches!(&d.change,
            Some(Change::NodeStatus(s)) if s.status == NodeStatusCode::Disconnected as i32))
    })
    .await;
    assert_eq!(pane_count(&server, &node_fp), 1, "last-known pane is kept");
    ui.send(preview(node_fp.as_str(), "%1", 43)).expect("send");
    let refused = ui_until(&mut ui, "unreachable", |e| {
        matches!(e.body, Some(ui_event_body::Body::Response(_)))
    })
    .await;
    assert!(matches!(
        refused.body,
        Some(ui_event_body::Body::Response(Response {
            request_id: 43,
            result: Some(response_result::Result::Error(_))
        }))
    ));

    // While away the node's world changed; reconnecting resyncs with a fresh snapshot.
    link.observe(vec![round(
        3,
        vec![obs("%1", 7, BUSY_SCREEN), obs("%2", 8, PERMIT_SCREEN)],
    )]);
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("resynced", || pane_count(&server, &node_fp) == 2).await;
    wait_until("online again", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    ui_until(&mut ui, "second pane", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d)) if matches!(&d.change,
            Some(Change::PaneUpsert(p)) if p.pane_ref.as_ref().is_some_and(|r| r.pane == "%2")))
    })
    .await;

    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_nodes_with_the_same_display_name_stay_distinct() {
    let (a, b) = (
        Arc::new(Identity::generate().expect("a")),
        Arc::new(Identity::generate().expect("b")),
    );
    let trust = trust_with(&[(&a, "studio"), (&b, "studio")], &[]);
    let (server, orch, addr) = start(trust, None).await;
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut runners = Vec::new();
    for id in [&a, &b] {
        let link = node_link(id, &addr, &orch, "studio", None, 1);
        let rx = stop_rx.clone();
        runners.push(tokio::spawn(async move { link.run(rx).await }));
    }
    wait_until("both online", || server.fleet_snapshot().nodes.len() == 2).await;
    let names: Vec<String> = server
        .fleet_snapshot()
        .nodes
        .iter()
        .map(|n| n.display_name.clone())
        .collect();
    assert_eq!(names, vec!["studio", "studio"]);
    stop_tx.send(true).expect("stop");
    for r in runners {
        r.await.expect("runner");
    }
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_workspaces_reach_the_ui_over_mutual_tls_and_survive_a_reconnect() {
    use flight_proto::{SavedHealthCode, SavedRootCode, SavedWorkspace};
    let saved = |key: &str, health: SavedHealthCode| SavedWorkspace {
        config_key: key.to_owned(),
        name: "nga".to_owned(),
        root: "/work/nga".to_owned(),
        health: health as i32,
        root_state: SavedRootCode::Missing as i32,
        detail: "no such directory".to_owned(),
        workspace_id: String::new(),
        imported: false,
    };
    let node_id = Arc::new(Identity::generate().expect("node"));
    let ui_id = Identity::generate().expect("ui");
    let trust = trust_with(&[(&node_id, "mini-1")], &[(&ui_id, "laptop")]);
    let (server, orch, addr) = start(trust, None).await;
    let node_fp = node_id.fingerprint().clone();
    let link = node_link(&node_id, &addr, &orch, "mini-1", None, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("node online", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    let mut ui = UiClient::connect(&addr, &ui_id, &orch).await.expect("ui");
    ui.send(subscribe()).expect("send");
    ui_until(&mut ui, "fleet snapshot", |e| {
        matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
    })
    .await;

    link.observe_saved(vec![saved("c-1", SavedHealthCode::Blocked)]);
    let got = ui_until(&mut ui, "saved delta", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d))
            if matches!(&d.change, Some(Change::NodeSaved(_))))
    })
    .await;
    let Some(ui_event_body::Body::Delta(d)) = got.body else {
        unreachable!()
    };
    let Some(Change::NodeSaved(n)) = d.change else {
        unreachable!()
    };
    assert_eq!(n.node_id, node_fp.as_str());
    assert_eq!(n.items[0].detail, "no such directory");

    // The node goes away; a UI that subscribes afterwards still sees the saved workspace.
    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    let mut late = UiClient::connect(&addr, &ui_id, &orch).await.expect("ui");
    late.send(subscribe()).expect("send");
    let first = ui_until(&mut late, "fleet snapshot", |e| {
        matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
    })
    .await;
    let Some(ui_event_body::Body::Snapshot(s)) = first.body else {
        unreachable!()
    };
    assert_eq!(s.nodes[0].saved.len(), 1);
    assert_eq!(s.nodes[0].status, NodeStatusCode::Disconnected as i32);
}
