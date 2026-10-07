// SPDX-License-Identifier: MIT

//! What a node tells its operator about its link: a refusal is reported (once, not once per
//! retry), and a good connection says so.

mod support;

use flight_node::{NodeCore, NodeSession};
use flight_transport::{NodeLink, NodeLinkConfig};
use flight_trust::{Identity, TrustStore};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::*;
use tokio::sync::watch;

fn link_with_log(
    identity: &Arc<Identity>,
    addr: &str,
    orch: &flight_trust::Fingerprint,
    lines: &Arc<Mutex<Vec<String>>>,
) -> Arc<NodeLink> {
    let session = NodeSession::new(NodeCore::new(identity.fingerprint().host_id(), inc(1)), "n");
    let sink = lines.clone();
    Arc::new(
        NodeLink::new(
            NodeLinkConfig {
                address: addr.to_owned(),
                identity: identity.clone(),
                orchestrator: orch.clone(),
                servers: vec!["flight".to_owned()],
                heartbeat_interval: Duration::from_millis(500),
                reconnect_min: Duration::from_millis(30),
                reconnect_max: Duration::from_millis(60),
            },
            session,
            Arc::new(PreviewControl),
        )
        .with_log(Arc::new(move |l| sink.lock().unwrap().push(l))),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_node_reports_why_once_however_often_it_retries() {
    let stranger = Arc::new(Identity::generate().expect("identity"));
    let (server, orch, addr) = start(TrustStore::empty(), None).await;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let link = link_with_log(&stranger, &addr, &orch, &lines);
    let (stop, rx) = watch::channel(false);
    let task = tokio::spawn({
        let link = link.clone();
        async move { link.run(rx).await }
    });
    tokio::time::sleep(Duration::from_millis(800)).await;
    let _ = stop.send(true);
    let _ = task.await;
    let got = lines.lock().unwrap().clone();
    assert_eq!(
        got.len(),
        1,
        "one report for many identical failures: {got:?}"
    );
    assert!(
        got[0].contains("link down") && got[0].contains("retrying"),
        "{got:?}"
    );
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusted_node_says_it_connected() {
    let node = Arc::new(Identity::generate().expect("identity"));
    let (server, orch, addr) = start(trust_with(&[(&node, "n")], &[]), None).await;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let link = link_with_log(&node, &addr, &orch, &lines);
    let (stop, rx) = watch::channel(false);
    let task = tokio::spawn({
        let link = link.clone();
        async move { link.run(rx).await }
    });
    wait_until("a connected report", || {
        lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("connected to "))
    })
    .await;
    let _ = stop.send(true);
    let _ = task.await;
    server.shutdown().await;
}
