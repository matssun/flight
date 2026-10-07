// SPDX-License-Identifier: MIT

//! Joining from a bundle, and the operator socket.

mod support;

use flight_proto::RoleCode;
use flight_transport::{
    admin_request, config_path, identity_dir, join, serve_admin, TransportError,
};
use flight_trust::{ConnectionConfig, EnrollmentBundle, Identity, Role, TrustStore};
use std::path::PathBuf;
use support::*;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

struct Rig {
    server: flight_transport::ServerHandle,
    admin: std::path::PathBuf,
    dir: PathBuf,
    _task: tokio::task::JoinHandle<()>,
}

async fn rig(name: &str) -> Rig {
    let dir = temp_dir(name);
    let (server, _orch, addr) = start(TrustStore::empty(), Some(dir.join("trust.toml"))).await;
    let admin = dir.join("admin.sock");
    let task = serve_admin(admin.clone(), server.clone_control(), addr).expect("admin");
    Rig {
        server,
        admin,
        dir,
        _task: task,
    }
}

async fn bundle(rig: &Rig) -> EnrollmentBundle {
    let text = admin_request(&rig.admin, "enroll 600")
        .await
        .expect("enroll");
    EnrollmentBundle::parse(&text).expect("bundle")
}

fn empty(dir: &std::path::Path) -> bool {
    !dir.exists() || std::fs::read_dir(dir).expect("read").next().is_none()
}

#[tokio::test(flavor = "multi_thread")]
async fn joining_writes_identity_and_connection_settings_and_the_node_is_trusted() {
    let rig = rig("join-ok").await;
    let role = rig.dir.join("node");
    let joined = join(&role, &bundle(&rig).await, "mini-1", RoleCode::Node, now())
        .await
        .expect("join");
    let identity = Identity::load(&identity_dir(&role)).expect("identity saved");
    assert_eq!(identity.fingerprint(), &joined.node_id);
    let config = ConnectionConfig::load(&config_path(&role)).expect("config saved");
    assert_eq!(config.display_name, "mini-1");
    assert_eq!(config.orchestrator().expect("fp"), joined.orchestrator);
    assert!(rig
        .server
        .trust()
        .is_authorized(identity.fingerprint(), Role::Node));
    rig.server.clone_control(); // still usable
    std::fs::remove_dir_all(&rig.dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_join_leaves_no_identity_and_no_trust_behind() {
    let rig = rig("join-fail").await;
    let good = bundle(&rig).await;

    // Wrong token.
    let role = rig.dir.join("node-a");
    let mut bad_token = good.clone();
    bad_token.token = "not-the-token".into();
    let err = join(&role, &bad_token, "x", RoleCode::Node, now())
        .await
        .unwrap_err();
    assert!(matches!(err, TransportError::Refused(_)), "{err:?}");
    assert!(empty(&role), "nothing may be written");

    // Wrong pinned orchestrator: the token is never sent, so it stays valid.
    let role_b = rig.dir.join("node-b");
    let mut wrong_pin = good.clone();
    wrong_pin.orchestrator = Identity::generate().expect("other").fingerprint().clone();
    let err = join(&role_b, &wrong_pin, "x", RoleCode::Node, now())
        .await
        .unwrap_err();
    assert!(matches!(err, TransportError::Connect(_)), "{err:?}");
    assert!(empty(&role_b));

    // Expired bundle: refused before any connection.
    let role_c = rig.dir.join("node-c");
    let mut stale = good.clone();
    stale.expires_at = 5;
    let err = join(&role_c, &stale, "x", RoleCode::Node, now())
        .await
        .unwrap_err();
    assert!(matches!(err, TransportError::Refused(_)));
    assert!(empty(&role_c));
    assert!(
        rig.server.trust().peers().is_empty(),
        "no trust was created"
    );

    // The token survived all of that.
    let role_d = rig.dir.join("node-d");
    join(&role_d, &good, "ok", RoleCode::Node, now())
        .await
        .expect("token still valid");
    std::fs::remove_dir_all(&rig.dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_identity_is_removed_again_if_the_settings_cannot_be_written() {
    let rig = rig("join-save").await;
    let role = rig.dir.join("node");
    // A directory where the settings file belongs makes the final atomic write fail.
    std::fs::create_dir_all(config_path(&role)).expect("blocker");
    let err = join(&role, &bundle(&rig).await, "x", RoleCode::Node, now()).await;
    assert!(err.is_err());
    assert!(
        !Identity::exists(&identity_dir(&role)),
        "no half-configured identity"
    );
    std::fs::remove_dir_all(&rig.dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_existing_identity_is_kept_when_a_rejoin_fails() {
    let rig = rig("join-keep").await;
    let role = rig.dir.join("node");
    let existing = Identity::load_or_create(&identity_dir(&role)).expect("identity");
    let mut bad = bundle(&rig).await;
    bad.token = "nope".into();
    assert!(join(&role, &bad, "x", RoleCode::Node, now()).await.is_err());
    let after = Identity::load(&identity_dir(&role)).expect("still there");
    assert_eq!(after.fingerprint(), existing.fingerprint());
    std::fs::remove_dir_all(&rig.dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operator_socket_lists_and_revokes_and_is_private() {
    let rig = rig("admin").await;
    let role = rig.dir.join("ui");
    let joined = join(&role, &bundle(&rig).await, "laptop", RoleCode::Ui, now())
        .await
        .expect("ui join");

    let listing = admin_request(&rig.admin, "trust").await.expect("trust");
    assert!(
        listing.contains(joined.node_id.as_str()) && listing.contains("ui enabled laptop"),
        "{listing}"
    );
    assert!(admin_request(&rig.admin, "status")
        .await
        .expect("status")
        .starts_with("nodes=0"));

    admin_request(&rig.admin, &format!("revoke {}", joined.node_id))
        .await
        .expect("revoke");
    assert!(!rig.server.trust().is_authorized(&joined.node_id, Role::Ui));
    assert!(admin_request(&rig.admin, "trust")
        .await
        .expect("trust")
        .contains("disabled"));

    assert!(matches!(
        admin_request(&rig.admin, "frobnicate").await,
        Err(TransportError::Refused(_))
    ));
    assert!(admin_request(&rig.admin, "revoke nonsense").await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&rig.admin)
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    std::fs::remove_dir_all(&rig.dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_orchestrator_is_reported_helpfully() {
    let err = admin_request(&std::env::temp_dir().join("flight-no-such.sock"), "status")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("flight orchestrator run"), "{err}");
}
