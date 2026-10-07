// SPDX-License-Identifier: MIT

use crate::connector::connect;
use crate::paths::ENROLL;
use crate::TransportError;
use flight_proto::{EnrollRequest, EnrollResponse, RoleCode};
use flight_trust::{Fingerprint, Identity};
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::Request;
use tonic_prost::ProstCodec;

/// Join the fleet with an enrollment token. The orchestrator is authenticated first, by its
/// pinned fingerprint: a server with any other key never receives the token.
pub async fn enroll(
    address: &str,
    identity: &Identity,
    orchestrator: &Fingerprint,
    token: &str,
    display_name: &str,
    role: RoleCode,
) -> Result<EnrollResponse, TransportError> {
    let channel = connect(address, identity, orchestrator).await?;
    let mut grpc = Grpc::new(channel);
    grpc.ready()
        .await
        .map_err(|e| TransportError::Connect(e.to_string()))?;
    let request = EnrollRequest {
        token: token.to_owned(),
        display_name: display_name.to_owned(),
        role: role as i32,
    };
    let response = grpc
        .unary(
            Request::new(request),
            PathAndQuery::from_static(ENROLL),
            ProstCodec::<EnrollRequest, EnrollResponse>::default(),
        )
        .await?;
    Ok(response.into_inner())
}
