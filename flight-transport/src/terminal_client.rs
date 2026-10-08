// SPDX-License-Identifier: MIT

use crate::connector::{connect_with, DEFAULT_DIAL_TIMEOUT};
use crate::paths::{TERMINAL_NODE, TERMINAL_UI};
use crate::TransportError;
use flight_proto::{Origin, TerminalFrame};
use flight_trust::{Fingerprint, Identity};
use std::time::Duration;
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::{Request, Streaming};
use tonic_prost::ProstCodec;

/// Frames queued toward the orchestrator. Sending waits for room, which is the backpressure.
const SEND_QUEUE: usize = 4;

/// One end of a terminal stream: the UI's or the node's. It dials the orchestrator on its own
/// connection (a terminal never shares one with fleet state), proves which terminal it is
/// with the id the orchestrator minted, and then exchanges [`TerminalFrame`]s.
pub struct TerminalClient {
    tx: Sender<TerminalFrame>,
    frames: Streaming<TerminalFrame>,
    me: Origin,
}

impl TerminalClient {
    /// The UI's end, after `OpenTerminal` answered with `terminal_id`.
    pub async fn connect_ui(
        address: &str,
        identity: &Identity,
        orchestrator: &Fingerprint,
        terminal_id: &[u8],
    ) -> Result<Self, TransportError> {
        Self::connect(
            address,
            identity,
            orchestrator,
            terminal_id,
            Origin::Ui,
            DEFAULT_DIAL_TIMEOUT,
        )
        .await
    }

    pub(crate) async fn connect(
        address: &str,
        identity: &Identity,
        orchestrator: &Fingerprint,
        terminal_id: &[u8],
        me: Origin,
        dial_timeout: Duration,
    ) -> Result<Self, TransportError> {
        let channel = connect_with(address, identity, orchestrator, dial_timeout).await?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        let (tx, rx) = mpsc::channel(SEND_QUEUE);
        tx.try_send(TerminalFrame::attach(terminal_id.to_vec()))
            .map_err(|_| TransportError::Protocol("cannot queue attach".to_owned()))?;
        let path = match me {
            Origin::Ui => TERMINAL_UI,
            Origin::Node => TERMINAL_NODE,
        };
        let response = tokio::time::timeout(
            dial_timeout * 2,
            grpc.streaming(
                Request::new(ReceiverStream::new(rx)),
                PathAndQuery::from_static(path),
                ProstCodec::<TerminalFrame, TerminalFrame>::default(),
            ),
        )
        .await
        .map_err(|_| {
            TransportError::Connect("the orchestrator did not accept the terminal".into())
        })??;
        Ok(Self {
            tx,
            frames: response.into_inner(),
            me,
        })
    }

    /// Send a frame, waiting for room. Refuses a frame this end may not send.
    pub async fn send(&self, frame: TerminalFrame) -> Result<(), TransportError> {
        frame
            .validate_from(self.me)
            .map_err(|e| TransportError::Protocol(e.to_string()))?;
        self.tx
            .send(frame)
            .await
            .map_err(|_| TransportError::Protocol("the terminal stream is closed".to_owned()))
    }

    /// Say goodbye properly: stop sending, then wait (briefly) for the other end to finish
    /// reading what was sent. Dropping a client with a final frame still queued can lose it.
    pub async fn finish(self, wait: Duration) {
        let Self { tx, mut frames, .. } = self;
        drop(tx);
        let _ = tokio::time::timeout(wait, async move {
            while let Ok(Some(_)) = frames.message().await {}
        })
        .await;
    }

    /// The next frame from the other end, or `None` when the stream ended. A frame the other
    /// end may not send is an error.
    pub async fn next(&mut self) -> Result<Option<TerminalFrame>, TransportError> {
        let peer = match self.me {
            Origin::Ui => Origin::Node,
            Origin::Node => Origin::Ui,
        };
        match self.frames.message().await? {
            Some(frame) => {
                frame
                    .validate_from(peer)
                    .map_err(|e| TransportError::Protocol(e.to_string()))?;
                Ok(Some(frame))
            }
            None => Ok(None),
        }
    }
}
