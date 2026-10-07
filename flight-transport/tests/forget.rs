// SPDX-License-Identifier: MIT

//! Forgetting a node over real sockets: refused while connected, removes the last-known
//! image for every UI once it is gone, and never touches trust.

mod support;

use flight_proto::{
    fleet_change::Change, ui_event_body, ui_request_body, NodeStatusCode, Subscribe, UiRequest,
};
use flight_transport::{TransportError, UiClient};
use flight_trust::Identity;
use std::sync::Arc;
use support::*;
use tokio::sync::watch;

fn subscribe() -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn forget_removes_a_gone_node_for_every_ui_and_leaves_trust_alone() {
    let node_id = Arc::new(Identity::generate().expect("node"));
    let ui_id = Identity::generate().expect("ui");
    let (server, orch, addr) = start(
        trust_with(&[(&node_id, "mini-1")], &[(&ui_id, "laptop")]),
        None,
    )
    .await;
    let node_fp = node_id.fingerprint().clone();

    let link = node_link(&node_id, &addr, &orch, "mini-1", None, 1);
    let (stop, rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(rx).await })
    };
    wait_until("node online", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    link.observe(vec![round(1, vec![obs("%1", 7, PERMIT_SCREEN)])]);
    wait_until("pane replicated", || pane_count(&server, &node_fp) == 1).await;

    let mut ui = UiClient::connect(&addr, &ui_id, &orch).await.expect("ui");
    ui.send(subscribe()).expect("send");
    ui_until(&mut ui, "snapshot", |e| {
        matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
    })
    .await;

    // Connected: refused, nothing changes.
    let refused = server.forget_node(node_fp.as_str());
    assert!(
        matches!(refused, Err(TransportError::Refused(ref m)) if m.contains("still connected")),
        "{refused:?}"
    );

    // Gone but known: last-known panes stay.
    let _ = stop.send(true);
    let _ = runner.await;
    wait_until("node disconnected", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Disconnected as i32)
    })
    .await;
    assert_eq!(
        pane_count(&server, &node_fp),
        1,
        "disconnect keeps the image"
    );

    // Forget: the UI is told, the node is gone from the fleet.
    server.forget_node(node_fp.as_str()).expect("forget");
    ui_until(&mut ui, "removal delta", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d))
            if matches!(&d.change, Some(Change::NodeRemoved(n)) if n.node_id == node_fp.as_str()))
    })
    .await;
    assert!(server.fleet_snapshot().nodes.is_empty());
    assert!(
        server
            .trust()
            .is_authorized(&node_fp, flight_trust::Role::Node),
        "forgetting is not revoking"
    );

    // A trusted node that comes back reappears, fresh.
    let again = node_link(&node_id, &addr, &orch, "mini-1", None, 2);
    let (stop2, rx2) = watch::channel(false);
    let runner2 = {
        let again = again.clone();
        tokio::spawn(async move { again.run(rx2).await })
    };
    wait_until("node back online", || {
        node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    assert_eq!(
        pane_count(&server, &node_fp),
        0,
        "no trace of the old image"
    );
    let _ = stop2.send(true);
    let _ = runner2.await;
    server.shutdown().await;
}
