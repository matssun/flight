// SPDX-License-Identifier: MIT

//! Dialling is always bounded: a peer that accepts the TCP connection and then says nothing
//! must not hold a node's connect attempt forever.

mod support;

use flight_node::{NodeCore, NodeSession};
use flight_transport::{NodeLink, NodeLinkConfig};
use flight_trust::Identity;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_accepts_and_never_answers_cannot_stall_a_dial() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let _silent = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = listener.accept().await {
            held.push(s); // accept, then never speak
        }
    });
    let identity = Arc::new(Identity::generate().unwrap());
    let orchestrator = Identity::generate().unwrap().fingerprint().clone();
    let session = NodeSession::new(NodeCore::new(identity.fingerprint().host_id(), inc(1)), "n");
    let link = NodeLink::new(
        NodeLinkConfig {
            address: addr,
            identity,
            orchestrator,
            servers: vec!["flight".to_owned()],
            heartbeat_interval: Duration::from_millis(500),
            reconnect_min: Duration::from_millis(30),
            reconnect_max: Duration::from_millis(60),
        },
        session,
        Arc::new(PreviewControl),
    )
    .with_dial_timeout(Duration::from_millis(300));
    let started = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(5), link.run_once())
        .await
        .expect("the dial must give up by itself");
    let err = result.expect_err("a silent peer is a failure");
    assert!(
        !err.is_unreachable(),
        "a silent peer is not a routing failure: {err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "bounded by the dial timeout, took {:?}",
        started.elapsed()
    );
}
