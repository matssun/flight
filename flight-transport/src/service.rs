// SPDX-License-Identifier: MIT

use crate::outbox::Outbox;
use crate::paths::{ENROLL, NODE_CONNECT, SERVICE_NAME, UI_CONNECT};
use crate::shared::{lock, now, SharedState};
use crate::PeerIdentity;
use flight_proto::{
    EnrollRequest, EnrollResponse, NodeFrame, OrchestratorFrame, RoleCode, UiEvent, UiRequest,
    Validate,
};
use flight_trust::Role;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};
use tonic::body::Body;
use tonic::server::{Grpc, StreamingService, UnaryService};
use tonic::{Request, Response, Status, Streaming};
use tonic_prost::ProstCodec;

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

/// The `flight.v1.Flight` gRPC service, hand-bound to tonic (no code generation).
#[derive(Clone)]
pub(crate) struct FlightService {
    state: SharedState,
}

impl FlightService {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }
}

impl tonic::server::NamedService for FlightService {
    const NAME: &'static str = SERVICE_NAME;
}

fn peer_of<T>(request: &Request<T>) -> Result<PeerIdentity, Status> {
    request
        .extensions()
        .get::<PeerIdentity>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("no authenticated peer"))
}

/// Feed an outbox to a gRPC response stream. One item is in flight at a time, so HTTP/2
/// flow control reaches the outbox and a slow reader makes it overflow (and resync) rather
/// than buffer. Ends when the outbox closes or the reader goes away.
fn pump<T: Send + 'static>(
    outbox: Arc<Outbox<T>>,
    resync: impl Fn() + Send + Sync + 'static,
) -> ReceiverStream<T> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        while let Some(item) = outbox.next(&resync).await {
            if tx.send(item).await.is_err() {
                outbox.close();
                break;
            }
        }
    });
    ReceiverStream::new(rx)
}

async fn node_connect(
    state: SharedState,
    request: Request<Streaming<NodeFrame>>,
) -> Result<Response<BoxStream<OrchestratorFrame>>, Status> {
    let peer = peer_of(&request)?;
    let (conn, outbox) = {
        let mut s = lock(&state);
        if !s.trust.is_authorized(&peer.0, Role::Node) {
            return Err(Status::permission_denied("not authorized"));
        }
        s.open_node(peer.0)
    };
    let mut inbound = request.into_inner();
    tokio::spawn(async move {
        while let Ok(Some(frame)) = inbound.message().await {
            let mut s = lock(&state);
            if !s.node_open(conn) {
                break;
            }
            s.on_node_frame(conn, frame);
        }
        lock(&state).close_node(conn);
    });
    let out: BoxStream<OrchestratorFrame> = Box::pin(pump(outbox, || {}).map(Ok));
    Ok(Response::new(out))
}

async fn ui_connect(
    state: SharedState,
    request: Request<Streaming<UiRequest>>,
) -> Result<Response<BoxStream<UiEvent>>, Status> {
    let peer = peer_of(&request)?;
    let (ui, outbox) = {
        let mut s = lock(&state);
        if !s.trust.is_authorized(&peer.0, Role::Ui) {
            return Err(Status::permission_denied("not authorized"));
        }
        s.open_ui(peer.0)
    };
    let mut inbound = request.into_inner();
    let resync_state = state.clone();
    tokio::spawn(async move {
        while let Ok(Some(req)) = inbound.message().await {
            let mut s = lock(&state);
            if !s.ui_open(ui) {
                break;
            }
            let fx = s.core.ui_request(ui, req, now());
            s.dispatch(fx);
        }
        lock(&state).close_ui(ui);
    });
    let out: BoxStream<UiEvent> =
        Box::pin(pump(outbox, move || lock(&resync_state).resync_ui(ui)).map(Ok));
    Ok(Response::new(out))
}

/// The only call an authenticated but not yet authorized identity may make. Every refusal
/// looks the same to the caller.
async fn enroll(
    state: SharedState,
    request: Request<EnrollRequest>,
) -> Result<Response<EnrollResponse>, Status> {
    let peer = peer_of(&request)?;
    let req = request.into_inner();
    req.validate()
        .map_err(|e| Status::invalid_argument(e.to_string()))?;
    let role = match RoleCode::try_from(req.role) {
        Ok(RoleCode::Ui) => Role::Ui,
        _ => Role::Node,
    };
    let mut s = lock(&state);
    if s.tokens.redeem(&req.token, now()).is_err() {
        return Err(Status::permission_denied("enrollment refused"));
    }
    let name: String = req.display_name.chars().take(128).collect();
    s.trust.authorize(&peer.0, &name, role);
    if let Some(path) = s.trust_path.clone() {
        s.trust
            .save(&path)
            .map_err(|e| Status::internal(format!("cannot record trust: {e}")))?;
    }
    Ok(Response::new(EnrollResponse {
        node_id: peer.0.to_string(),
        orchestrator_id: s.orchestrator_id.to_string(),
    }))
}

struct NodeConnectSvc(SharedState);

impl StreamingService<NodeFrame> for NodeConnectSvc {
    type Response = OrchestratorFrame;
    type ResponseStream = BoxStream<OrchestratorFrame>;
    type Future = BoxFuture<Result<Response<Self::ResponseStream>, Status>>;

    fn call(&mut self, request: Request<Streaming<NodeFrame>>) -> Self::Future {
        Box::pin(node_connect(self.0.clone(), request))
    }
}

struct UiConnectSvc(SharedState);

impl StreamingService<UiRequest> for UiConnectSvc {
    type Response = UiEvent;
    type ResponseStream = BoxStream<UiEvent>;
    type Future = BoxFuture<Result<Response<Self::ResponseStream>, Status>>;

    fn call(&mut self, request: Request<Streaming<UiRequest>>) -> Self::Future {
        Box::pin(ui_connect(self.0.clone(), request))
    }
}

struct EnrollSvc(SharedState);

impl UnaryService<EnrollRequest> for EnrollSvc {
    type Response = EnrollResponse;
    type Future = BoxFuture<Result<Response<EnrollResponse>, Status>>;

    fn call(&mut self, request: Request<EnrollRequest>) -> Self::Future {
        Box::pin(enroll(self.0.clone(), request))
    }
}

impl<B> tower::Service<http::Request<B>> for FlightService
where
    B: http_body::Body + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let state = self.state.clone();
        match req.uri().path() {
            NODE_CONNECT => Box::pin(async move {
                let mut grpc = Grpc::new(ProstCodec::<OrchestratorFrame, NodeFrame>::default());
                Ok(grpc.streaming(NodeConnectSvc(state), req).await)
            }),
            UI_CONNECT => Box::pin(async move {
                let mut grpc = Grpc::new(ProstCodec::<UiEvent, UiRequest>::default());
                Ok(grpc.streaming(UiConnectSvc(state), req).await)
            }),
            ENROLL => Box::pin(async move {
                let mut grpc = Grpc::new(ProstCodec::<EnrollResponse, EnrollRequest>::default());
                Ok(grpc.unary(EnrollSvc(state), req).await)
            }),
            _ => Box::pin(async move { Ok(Status::unimplemented("unknown method").into_http()) }),
        }
    }
}
