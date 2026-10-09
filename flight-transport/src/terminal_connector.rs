// SPDX-License-Identifier: MIT

use crate::connector::{connect_with, DEFAULT_DIAL_TIMEOUT};
use crate::{TerminalClient, TransportError};
use flight_proto::Origin;
use flight_trust::{Fingerprint, Identity};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::transport::Channel;

/// A process's connection for terminal streams, kept between terminals.
///
/// Every terminal used to dial its own mutually authenticated connection; a switch between
/// surfaces paid for a TCP and TLS handshake each time, from each end. The streams of an HTTP/2
/// connection are independent, so a terminal is one more stream on a connection that is already
/// up. Terminals still never share a connection with fleet state: a terminal flooding output
/// must not hold up replication.
///
/// The connection is dialed on first use and kept. If it is gone when a terminal is wanted
/// (the orchestrator restarted, the network dropped) one fresh dial is made before giving up;
/// a connection that has gone quiet is found by the HTTP/2 keep-alive and by the bound on
/// opening a stream.
pub struct TerminalConnector {
    address: String,
    identity: Arc<Identity>,
    orchestrator: Fingerprint,
    dial_timeout: Duration,
    channel: Mutex<Option<Channel>>,
}

impl TerminalConnector {
    pub fn new(address: &str, identity: Arc<Identity>, orchestrator: Fingerprint) -> Self {
        Self::with_dial_timeout(address, identity, orchestrator, DEFAULT_DIAL_TIMEOUT)
    }

    pub fn with_dial_timeout(
        address: &str,
        identity: Arc<Identity>,
        orchestrator: Fingerprint,
        dial_timeout: Duration,
    ) -> Self {
        Self {
            address: address.to_owned(),
            identity,
            orchestrator,
            dial_timeout,
            channel: Mutex::new(None),
        }
    }

    /// The UI's end of terminal `terminal_id`.
    pub async fn connect_ui(&self, terminal_id: &[u8]) -> Result<TerminalClient, TransportError> {
        self.open(terminal_id, Origin::Ui).await
    }

    /// The node's end of terminal `terminal_id`.
    pub(crate) async fn connect_node(
        &self,
        terminal_id: &[u8],
    ) -> Result<TerminalClient, TransportError> {
        self.open(terminal_id, Origin::Node).await
    }

    async fn open(&self, terminal_id: &[u8], me: Origin) -> Result<TerminalClient, TransportError> {
        if let Some(channel) = self.cached().await {
            match TerminalClient::attach_on(channel, terminal_id, me, self.dial_timeout).await {
                Ok(client) => return Ok(client),
                // The kept connection is no good any more; forget it and dial once.
                Err(_) => self.forget().await,
            }
        }
        let channel = self.fresh().await?;
        TerminalClient::attach_on(channel, terminal_id, me, self.dial_timeout).await
    }

    async fn cached(&self) -> Option<Channel> {
        self.channel.lock().await.clone()
    }

    async fn forget(&self) {
        *self.channel.lock().await = None;
    }

    async fn fresh(&self) -> Result<Channel, TransportError> {
        let mut slot = self.channel.lock().await;
        if let Some(channel) = slot.as_ref() {
            // Another caller dialed while this one waited for the lock.
            return Ok(channel.clone());
        }
        let channel = connect_with(
            &self.address,
            &self.identity,
            &self.orchestrator,
            self.dial_timeout,
        )
        .await?;
        *slot = Some(channel.clone());
        Ok(channel)
    }
}
