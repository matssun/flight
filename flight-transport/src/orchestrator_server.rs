// SPDX-License-Identifier: MIT

use crate::incoming::accept_tls;
use crate::service::FlightService;
use crate::shared::{lock, now, Shared, SharedState};
use crate::TransportError;
use flight_orchestrator::{OrchestratorConfig, OrchestratorCore};
use flight_proto::{FleetSnapshot, Incarnation};
use flight_trust::{server_config, Fingerprint, Identity, IssuedToken, Role, TrustStore};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tonic::transport::Server;

pub struct ServerConfig {
    pub bind: SocketAddr,
    pub identity: Identity,
    pub trust: TrustStore,
    /// Where accepted enrollments and revocations are written; `None` keeps them in memory.
    pub trust_path: Option<PathBuf>,
    pub core: OrchestratorConfig,
    /// Fresh per orchestrator process.
    pub incarnation: Incarnation,
    pub tick_interval: Duration,
}

/// Operator controls for a running orchestrator. Cheap to clone; the admin socket and the
/// embedding process each hold one.
#[derive(Clone)]
pub struct ServerControl {
    state: SharedState,
    local_addr: SocketAddr,
}

/// A running orchestrator: the gRPC server and its ticker.
pub struct ServerHandle {
    control: ServerControl,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl std::ops::Deref for ServerHandle {
    type Target = ServerControl;

    fn deref(&self) -> &ServerControl {
        &self.control
    }
}

pub async fn serve(config: ServerConfig) -> Result<ServerHandle, TransportError> {
    let listener = TcpListener::bind(config.bind).await?;
    let local_addr = listener.local_addr()?;
    let tls = Arc::new(server_config(&config.identity)?);
    let orchestrator_id = config.identity.fingerprint().clone();
    let state: SharedState = Arc::new(Mutex::new(Shared::new(
        OrchestratorCore::new(config.core, config.incarnation),
        config.trust,
        config.trust_path,
        orchestrator_id,
    )));
    let (stop, stop_rx) = watch::channel(false);
    let incoming = accept_tls(listener, tls, stop_rx.clone());
    let mut shutdown = stop_rx.clone();
    let server = Server::builder()
        .http2_keepalive_interval(Some(Duration::from_secs(10)))
        .http2_keepalive_timeout(Some(Duration::from_secs(10)))
        .add_service(FlightService::new(state.clone()))
        .serve_with_incoming_shutdown(incoming, async move {
            let _ = shutdown.changed().await;
        });
    let server_task = tokio::spawn(async move {
        let _ = server.await;
    });
    let ticker_state = state.clone();
    let mut ticker_stop = stop_rx;
    let tick_interval = config.tick_interval;
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tick_interval);
        loop {
            tokio::select! {
                _ = ticker_stop.changed() => break,
                _ = interval.tick() => lock(&ticker_state).tick(),
            }
        }
    });
    Ok(ServerHandle {
        control: ServerControl { state, local_addr },
        stop,
        tasks: vec![server_task, ticker],
    })
}

impl ServerControl {
    /// Another handle to the same running orchestrator.
    pub fn clone_control(&self) -> ServerControl {
        self.clone()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn fingerprint(&self) -> Fingerprint {
        lock(&self.state).orchestrator_id.clone()
    }

    /// Issue a single-use enrollment token (held hashed, in memory).
    pub fn create_enrollment(&self, ttl_secs: u64) -> Result<IssuedToken, TransportError> {
        Ok(lock(&self.state).tokens.issue(now(), ttl_secs)?)
    }

    pub fn fleet_snapshot(&self) -> FleetSnapshot {
        lock(&self.state).core.fleet_snapshot()
    }

    /// The longest outbound queue to any peer: a gauge for "is someone falling behind".
    /// Never exceeds the queue capacity; overflow means resync, not buffering.
    pub fn max_backlog(&self) -> usize {
        lock(&self.state).max_backlog()
    }

    /// Terminals open right now (any state).
    pub fn terminals_open(&self) -> usize {
        lock(&self.state).terminals_open()
    }

    /// Relays (queues plus abort signal) still held; zero once every terminal has finished.
    pub fn terminal_relays(&self) -> usize {
        lock(&self.state).terminal_relays()
    }

    /// The most frames ever queued in one terminal direction inside the orchestrator. Bounded
    /// by design; exposed so a test can assert it.
    pub fn terminal_queue_peak(&self) -> usize {
        lock(&self.state).terminal_queue_peak()
    }

    /// How long a terminal side may be unable to move a frame before the terminal ends
    /// (default 30 s).
    pub fn set_terminal_stall(&self, stall: Duration) {
        lock(&self.state).set_terminal_stall(stall);
    }

    pub fn trust(&self) -> TrustStore {
        lock(&self.state).trust.clone()
    }

    /// Authorize an identity directly (the operator's `trust add`).
    pub fn authorize(
        &self,
        id: &Fingerprint,
        display_name: &str,
        role: Role,
    ) -> Result<(), TransportError> {
        let mut s = lock(&self.state);
        s.trust.authorize(id, display_name, role);
        save(&s)
    }

    /// Report liveness changes and connection closes (with ages and reasons) to `log`.
    pub fn set_log(&self, log: crate::LinkLog) {
        lock(&self.state).set_log(log);
    }

    /// Forget a disconnected node: remove its last-known image from the fleet and tell every
    /// UI. Separate from [`revoke`](Self::revoke): trust is untouched, so a trusted node that
    /// connects again reappears fresh. Refused while the node has a live connection.
    pub fn forget_node(&self, node_id: &str) -> Result<(), TransportError> {
        let mut s = lock(&self.state);
        let fx = s
            .core
            .forget_node(&flight_state::HostId::new(node_id))
            .map_err(|e| TransportError::Refused(e.to_string()))?;
        s.dispatch(fx);
        Ok(())
    }

    /// Revoke an identity: disable its entry and drop its live connections.
    pub fn revoke(&self, id: &Fingerprint) -> Result<(), TransportError> {
        let mut s = lock(&self.state);
        s.trust.disable(id);
        save(&s)?;
        s.enforce_trust();
        Ok(())
    }
}

impl ServerHandle {
    /// Stop serving. Open streams are ended first; a server that still has not finished
    /// after a short grace period is aborted.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        lock(&self.control.state).close_all();
        for mut task in self.tasks {
            if tokio::time::timeout(Duration::from_secs(1), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }
}

fn save(s: &Shared) -> Result<(), TransportError> {
    if let Some(path) = &s.trust_path {
        s.trust.save(path)?;
    }
    Ok(())
}
