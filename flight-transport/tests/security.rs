// SPDX-License-Identifier: MIT

//! The security story end to end on localhost: who may connect, who may enroll, and what
//! happens when trust changes.

mod support;

use flight_proto::{NodeStatusCode, RoleCode};
use flight_state::HostId;
use flight_transport::{enroll, TransportError, UiClient};
use flight_trust::{Identity, Role, TrustStore};
use std::sync::Arc;
use support::*;
use tokio::sync::watch;

fn id() -> Arc<Identity> {
    Arc::new(Identity::generate().expect("identity"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_certificate_with_an_unknown_node_id_is_refused() {
    let stranger = id();
    let (server, orch, addr) = start(TrustStore::empty(), None).await;
    let link = node_link(&stranger, &addr, &orch, "stranger", None, 1);
    let result = within("refusal", link.run_once()).await;
    assert!(
        matches!(result, Err(TransportError::Refused(_))),
        "{result:?}"
    );
    assert!(server.fleet_snapshot().nodes.is_empty());
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hello_claiming_a_different_node_id_than_the_key_is_refused() {
    let (real, other) = (id(), id());
    let (server, orch, addr) = start(trust_with(&[(&real, "mini-1")], &[]), None).await;
    // The session claims another node's id while authenticating with `real`'s key.
    let link = node_link(
        &real,
        &addr,
        &orch,
        "liar",
        Some(HostId::new(other.fingerprint().as_str())),
        1,
    );
    let _ = within("stream to end", link.run_once()).await;
    assert!(
        server.fleet_snapshot().nodes.is_empty(),
        "no node may register under a forged id"
    );
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_refuses_an_orchestrator_with_the_wrong_fingerprint() {
    let node = id();
    let (server, _orch, addr) = start(trust_with(&[(&node, "mini-1")], &[]), None).await;
    let wrong = Identity::generate().expect("other").fingerprint().clone();
    let link = node_link(&node, &addr, &wrong, "mini-1", None, 1);
    let result = within("connect failure", link.run_once()).await;
    assert!(
        matches!(result, Err(TransportError::Connect(_))),
        "{result:?}"
    );
    assert!(server.fleet_snapshot().nodes.is_empty());
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn enrollment_authorizes_the_node_once_with_a_valid_token() {
    let node = id();
    let dir = temp_dir("enroll");
    let trust_path = dir.join("trust.toml");
    let (server, orch, addr) = start(TrustStore::empty(), Some(trust_path.clone())).await;
    let token = server.create_enrollment(600).expect("token");

    // Not yet trusted: the stream is refused.
    let link = node_link(&node, &addr, &orch, "mini-1", None, 1);
    assert!(matches!(
        within("pre", link.run_once()).await,
        Err(TransportError::Refused(_))
    ));

    let reply = within(
        "enroll",
        enroll(&addr, &node, &orch, &token.secret, "mini-1", RoleCode::Node),
    )
    .await
    .expect("enrolled");
    assert_eq!(reply.node_id, node.fingerprint().as_str());
    assert_eq!(reply.orchestrator_id, orch.as_str());

    // The same token cannot enroll anyone again.
    let second = id();
    let replay = within(
        "replay",
        enroll(&addr, &second, &orch, &token.secret, "evil", RoleCode::Node),
    )
    .await;
    assert!(
        matches!(replay, Err(TransportError::Refused(_))),
        "{replay:?}"
    );
    assert!(!server
        .trust()
        .is_authorized(second.fingerprint(), Role::Node));

    // The enrolled node now connects, and the decision was written to trust.toml without the token.
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    let fp = node.fingerprint().clone();
    wait_until("enrolled node online", || {
        node_status(&server, &fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    let on_disk = TrustStore::load(&trust_path).expect("trust file");
    assert!(on_disk.is_authorized(node.fingerprint(), Role::Node));
    assert!(!std::fs::read_to_string(&trust_path)
        .expect("read")
        .contains(&token.secret));

    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    server.shutdown().await;
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_token_is_refused() {
    let node = id();
    let (server, orch, addr) = start(TrustStore::empty(), None).await;
    let token = server.create_enrollment(0).expect("token");
    let result = within(
        "expired",
        enroll(&addr, &node, &orch, &token.secret, "late", RoleCode::Node),
    )
    .await;
    assert!(
        matches!(result, Err(TransportError::Refused(_))),
        "{result:?}"
    );
    assert!(!server.trust().is_authorized(node.fingerprint(), Role::Node));
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_orchestrator_fingerprint_never_receives_the_token() {
    let node = id();
    let (server, orch, addr) = start(TrustStore::empty(), None).await;
    let token = server.create_enrollment(600).expect("token");
    let wrong = Identity::generate().expect("w").fingerprint().clone();
    let refused = within(
        "wrong pin",
        enroll(&addr, &node, &wrong, &token.secret, "n", RoleCode::Node),
    )
    .await;
    assert!(
        matches!(refused, Err(TransportError::Connect(_))),
        "{refused:?}"
    );
    // The token was not spent: the correct pin still enrolls with it.
    within(
        "right pin",
        enroll(&addr, &node, &orch, &token.secret, "n", RoleCode::Node),
    )
    .await
    .expect("token still valid");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_or_removed_node_is_refused_and_revocation_drops_live_connections() {
    let node = id();
    let (server, orch, addr) = start(trust_with(&[(&node, "mini-1")], &[]), None).await;
    let fp = node.fingerprint().clone();
    let link = node_link(&node, &addr, &orch, "mini-1", None, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("online", || {
        node_status(&server, &fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;

    server.revoke(&fp).expect("revoke");
    wait_until("dropped", || {
        node_status(&server, &fp) == Some(NodeStatusCode::Disconnected as i32)
    })
    .await;
    // Reconnect attempts keep failing while it is disabled.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        node_status(&server, &fp),
        Some(NodeStatusCode::Disconnected as i32)
    );
    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");

    let again = node_link(&node, &addr, &orch, "mini-1", None, 1);
    assert!(matches!(
        within("disabled", again.run_once()).await,
        Err(TransportError::Refused(_))
    ));
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_key_under_an_old_name_is_a_different_node_and_not_trusted() {
    let (old, replacement) = (id(), id());
    let (server, orch, addr) = start(trust_with(&[(&old, "mini-1")], &[]), None).await;
    let impostor = node_link(&replacement, &addr, &orch, "mini-1", None, 1);
    assert!(matches!(
        within("refused", impostor.run_once()).await,
        Err(TransportError::Refused(_))
    ));
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn roles_are_separate_a_node_cannot_be_a_ui_and_a_ui_cannot_be_a_node() {
    let (node, ui) = (id(), id());
    let (server, orch, addr) =
        start(trust_with(&[(&node, "mini-1")], &[(&ui, "laptop")]), None).await;
    let as_ui = UiClient::connect(&addr, &node, &orch).await;
    assert!(
        matches!(as_ui, Err(TransportError::Refused(_))),
        "node key as UI"
    );
    let ui_as_node = node_link(&ui, &addr, &orch, "laptop", None, 1);
    assert!(matches!(
        within("refused", ui_as_node.run_once()).await,
        Err(TransportError::Refused(_))
    ));
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unauthorized_identity_can_do_nothing_but_enroll() {
    let stranger = id();
    let (server, orch, addr) = start(TrustStore::empty(), None).await;
    assert!(matches!(
        UiClient::connect(&addr, &stranger, &orch).await,
        Err(TransportError::Refused(_))
    ));
    let bogus = within(
        "bogus",
        enroll(&addr, &stranger, &orch, "not-a-token", "x", RoleCode::Node),
    )
    .await;
    assert!(matches!(bogus, Err(TransportError::Refused(_))));
    server.shutdown().await;
}
