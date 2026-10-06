// SPDX-License-Identifier: MIT

use crate::connector::connect;
use crate::paths::NODE_CONNECT;
use crate::shared::now;
use crate::TransportError;
use flight_node::{Control, NodeSession, Round};
use flight_proto::{NodeFrame, OrchestratorFrame};
use flight_trust::{Fingerprint, Identity};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::watch;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::Request;
use tonic_prost::ProstCodec;

pub struct NodeLinkConfig {
    /// `host:port` of the orchestrator.
    pub address: String,
    pub identity: Arc<Identity>,
    /// The orchestrator's pinned identity.
    pub orchestrator: Fingerprint,
    /// The tmux servers this node observes (advertised in the hello).
    pub servers: Vec<String>,
    pub heartbeat_interval: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
}

type Out = Arc<Mutex<Option<UnboundedSender<NodeFrame>>>>;

/// Clears the outbound sender however `run_once` ends, including when its future is dropped
/// mid-flight: a lingering sender would keep the stream open after the link was stopped.
struct ClearOnDrop(Out);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        *guard(&self.0) = None;
    }
}

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A node's connection to the orchestrator: one long-lived outbound bidirectional stream,
/// re-dialled with backoff. The protocol itself lives in the `NodeSession`.
pub struct NodeLink<C: Control + Send + 'static> {
    cfg: NodeLinkConfig,
    session: Arc<Mutex<NodeSession<C>>>,
    out: Out,
}

impl<C: Control + Send + 'static> NodeLink<C> {
    pub fn new(cfg: NodeLinkConfig, session: NodeSession<C>) -> Self {
        Self {
            cfg,
            session: Arc::new(Mutex::new(session)),
            out: Arc::default(),
        }
    }

    /// Fold observations into the node's state and stream the resulting deltas, if connected.
    /// The session lock is held while sending, so frames leave in the order they were made.
    pub fn observe(&self, rounds: Vec<Round>) {
        let mut session = guard(&self.session);
        let frames = session.observe(rounds);
        if let Some(tx) = guard(&self.out).as_ref() {
            for f in frames {
                let _ = tx.send(f);
            }
        }
    }

    /// Read access to the session, for inspection.
    pub fn with_session<T>(&self, f: impl FnOnce(&NodeSession<C>) -> T) -> T {
        f(&guard(&self.session))
    }

    /// Keep connected until `stop` flips, re-dialling with exponential backoff.
    pub async fn run(&self, mut stop: watch::Receiver<bool>) {
        let mut delay = self.cfg.reconnect_min;
        loop {
            let started = std::time::Instant::now();
            tokio::select! {
                _ = stop.changed() => return,
                _ = self.run_once() => {}
            }
            if started.elapsed() > self.cfg.reconnect_max {
                delay = self.cfg.reconnect_min;
            }
            tokio::select! {
                _ = stop.changed() => return,
                _ = tokio::time::sleep(delay) => {}
            }
            delay = (delay * 2).min(self.cfg.reconnect_max);
        }
    }

    /// One connection: dial, handshake, serve until the stream ends.
    pub async fn run_once(&self) -> Result<(), TransportError> {
        let channel = connect(
            &self.cfg.address,
            &self.cfg.identity,
            &self.cfg.orchestrator,
        )
        .await?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        let (tx, rx) = unbounded_channel::<NodeFrame>();
        {
            let mut session = guard(&self.session);
            let _ = tx.send(session.connect(self.cfg.servers.clone()));
            *guard(&self.out) = Some(tx.clone());
        }
        let _clear = ClearOnDrop(self.out.clone());
        let outbound = UnboundedReceiverStream::new(rx);
        self.serve_stream(&mut grpc, outbound, &tx).await
    }

    async fn serve_stream(
        &self,
        grpc: &mut Grpc<tonic::transport::Channel>,
        outbound: UnboundedReceiverStream<NodeFrame>,
        tx: &UnboundedSender<NodeFrame>,
    ) -> Result<(), TransportError> {
        let codec = ProstCodec::<NodeFrame, OrchestratorFrame>::default();
        let response = grpc
            .streaming(
                Request::new(outbound),
                PathAndQuery::from_static(NODE_CONNECT),
                codec,
            )
            .await?;
        let mut inbound = response.into_inner();
        let mut beat = tokio::time::interval(self.cfg.heartbeat_interval);
        let mut seq = 0u64;
        loop {
            tokio::select! {
                _ = beat.tick() => {
                    seq += 1;
                    let _ = tx.send(guard(&self.session).heartbeat(seq));
                }
                message = inbound.message() => {
                    let Some(frame) = message? else { return Ok(()) };
                    let mut session = guard(&self.session);
                    let out = session.on_frame(frame, now());
                    for f in out.frames {
                        let _ = tx.send(f);
                    }
                    if out.close.is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
