// SPDX-License-Identifier: MIT

//! The lease of a terminal presentation (ADR-004): how it tells the orchestrator it is alive.

use crate::ClientConfig;
use flight_proto::{ui_request_body, TerminalLease, UiRequest};
use flight_transport::UiClient;
use std::time::Duration;

/// How often a presentation renews its terminal's lease. The orchestrator lets a lease run
/// for three of these.
pub const LEASE_PERIOD: Duration = Duration::from_secs(5);

/// How a presentation tells the orchestrator it is alive. `renew` runs from the relay's own
/// loop, so it stops being called exactly when the relay stops being serviced; it returns an
/// error when the control connection it rides on is gone.
pub struct Lease {
    pub period: Duration,
    pub renew: Box<dyn FnMut() -> Result<(), String> + Send>,
}

impl Lease {
    /// A lease for terminal `terminal_id` on a control connection of its own: the dashboard's
    /// goes away with the dashboard. It subscribes to nothing; it only says "still here".
    pub async fn connect(config: &ClientConfig, terminal_id: &[u8]) -> Result<Self, String> {
        let control = UiClient::connect(&config.address, &config.identity, &config.orchestrator)
            .await
            .map_err(|e| format!("no control connection: {e}"))?;
        let id = terminal_id.to_vec();
        Ok(Self {
            period: LEASE_PERIOD,
            renew: Box::new(move || {
                control
                    .send(UiRequest {
                        body: Some(ui_request_body::Body::TerminalLease(TerminalLease {
                            terminal_id: id.clone(),
                        })),
                    })
                    .map_err(|e| e.to_string())
            }),
        })
    }
}
