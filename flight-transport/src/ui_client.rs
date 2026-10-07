// SPDX-License-Identifier: MIT

use crate::connector::connect;
use crate::paths::UI_CONNECT;
use crate::TransportError;
use flight_proto::{UiEvent, UiRequest};
use flight_trust::{Fingerprint, Identity};
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::{Request, Streaming};
use tonic_prost::ProstCodec;

/// Requests a UI may have waiting to be sent.
const PENDING_REQUESTS: usize = 64;

/// A UI's connection to the orchestrator: send `UiRequest`s, read `UiEvent`s.
pub struct UiClient {
    tx: Sender<UiRequest>,
    events: Streaming<UiEvent>,
}

impl UiClient {
    pub async fn connect(
        address: &str,
        identity: &Identity,
        orchestrator: &Fingerprint,
    ) -> Result<Self, TransportError> {
        let channel = connect(address, identity, orchestrator).await?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        let (tx, rx) = mpsc::channel(PENDING_REQUESTS);
        let codec = ProstCodec::<UiRequest, UiEvent>::default();
        let response = grpc
            .streaming(
                Request::new(ReceiverStream::new(rx)),
                PathAndQuery::from_static(UI_CONNECT),
                codec,
            )
            .await?;
        Ok(Self {
            tx,
            events: response.into_inner(),
        })
    }

    /// Queue a request. Bounded: fails if the orchestrator is not reading.
    pub fn send(&self, request: UiRequest) -> Result<(), TransportError> {
        self.tx
            .try_send(request)
            .map_err(|_| TransportError::Protocol("too many requests pending".to_owned()))
    }

    /// The next event, or `None` when the orchestrator closed the stream.
    pub async fn next_event(&mut self) -> Result<Option<UiEvent>, TransportError> {
        Ok(self.events.message().await?)
    }
}
