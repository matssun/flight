// SPDX-License-Identifier: MIT

use crate::connector::connect;
use crate::paths::{NODE_CONNECT, UI_CONNECT};
use crate::{enroll, TransportError};
use flight_proto::{NodeFrame, OrchestratorFrame, RoleCode, UiEvent, UiRequest};
use flight_trust::{ConnectionConfig, EnrollmentBundle, Fingerprint, Identity};
use std::path::Path;
use tokio_stream::wrappers::ReceiverStream;
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::Request;
use tonic_prost::ProstCodec;

const IDENTITY_DIR: &str = "identity";
const CONFIG_FILE: &str = "connection.toml";

/// Where a role keeps its identity and connection settings.
pub fn identity_dir(role_dir: &Path) -> std::path::PathBuf {
    role_dir.join(IDENTITY_DIR)
}

pub fn config_path(role_dir: &Path) -> std::path::PathBuf {
    role_dir.join(CONFIG_FILE)
}

/// What a successful join established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    pub node_id: Fingerprint,
    pub orchestrator: Fingerprint,
}

/// Open the role's stream just long enough to learn whether the orchestrator accepts this
/// identity for it (refusal surfaces as `Refused`).
pub async fn probe(
    address: &str,
    identity: &Identity,
    orchestrator: &Fingerprint,
    role: RoleCode,
) -> Result<(), TransportError> {
    let channel = connect(address, identity, orchestrator).await?;
    let mut grpc = Grpc::new(channel);
    grpc.ready()
        .await
        .map_err(|e| TransportError::Connect(e.to_string()))?;
    // An outbound stream that ends at once: the call is accepted or refused, nothing more.
    match role {
        RoleCode::Ui => {
            let (tx, rx) = tokio::sync::mpsc::channel::<UiRequest>(1);
            drop(tx);
            grpc.streaming(
                Request::new(ReceiverStream::new(rx)),
                PathAndQuery::from_static(UI_CONNECT),
                ProstCodec::<UiRequest, UiEvent>::default(),
            )
            .await?;
        }
        _ => {
            let (tx, rx) = tokio::sync::mpsc::channel::<NodeFrame>(1);
            drop(tx);
            grpc.streaming(
                Request::new(ReceiverStream::new(rx)),
                PathAndQuery::from_static(NODE_CONNECT),
                ProstCodec::<NodeFrame, OrchestratorFrame>::default(),
            )
            .await?;
        }
    }
    Ok(())
}

/// Join an orchestrator from an enrollment bundle, leaving nothing behind on failure.
///
/// 1. the bundle must not have expired;
/// 2. connect only to the stated address, authenticating the orchestrator by the pinned
///    fingerprint (the token is not sent to anything else);
/// 3. present the token and this identity (generated in memory if the role has none yet);
/// 4. check the reply names this identity and that orchestrator, then prove the
///    orchestrator really accepts it by opening the role's stream;
/// 5. only then write the identity (if new) and the connection settings, atomically.
///
/// If writing fails, a newly created identity is removed again.
pub async fn join(
    role_dir: &Path,
    bundle: &EnrollmentBundle,
    display_name: &str,
    role: RoleCode,
    now: u64,
) -> Result<Joined, TransportError> {
    if bundle.expires_at <= now {
        return Err(TransportError::Refused(
            "the enrollment bundle has expired".to_owned(),
        ));
    }
    let id_dir = identity_dir(role_dir);
    let existing = Identity::exists(&id_dir);
    let identity = if existing {
        Identity::load(&id_dir)?
    } else {
        Identity::generate()?
    };

    let reply = enroll(
        &bundle.address,
        &identity,
        &bundle.orchestrator,
        &bundle.token,
        display_name,
        role,
    )
    .await?;
    if reply.node_id != identity.fingerprint().as_str()
        || reply.orchestrator_id != bundle.orchestrator.as_str()
    {
        return Err(TransportError::Protocol(
            "the orchestrator's answer does not match this enrollment".to_owned(),
        ));
    }
    probe(&bundle.address, &identity, &bundle.orchestrator, role).await?;

    if !existing {
        identity.save(&id_dir)?;
    }
    let config = ConnectionConfig::new(&bundle.orchestrator, &bundle.address, display_name);
    if let Err(e) = config.save(&config_path(role_dir)) {
        if !existing {
            Identity::remove(&id_dir);
        }
        return Err(e.into());
    }
    Ok(Joined {
        node_id: identity.fingerprint().clone(),
        orchestrator: bundle.orchestrator.clone(),
    })
}
