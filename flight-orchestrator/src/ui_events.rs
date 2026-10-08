// SPDX-License-Identifier: MIT

use crate::{Effects, OrchestratorCore, UiId};
use flight_proto::{ui_request_body, ErrorKindCode, UiRequest};

impl OrchestratorCore {
    /// Start (or restart) a UI's stream: a fresh `FleetSnapshot`, then deltas from sequence 1.
    pub fn subscribe(&mut self, ui: UiId) -> Effects {
        let snapshot = self.fleet_snapshot();
        let mut fx = Effects::default();
        fx.to_ui.push((ui, self.hub.subscribe(ui, snapshot)));
        fx
    }

    pub fn ui_disconnected(&mut self, ui: UiId) {
        self.hub.unsubscribe(ui);
    }

    pub fn ui_request(&mut self, ui: UiId, request: UiRequest, now: u64) -> Effects {
        match request.body {
            Some(ui_request_body::Body::Subscribe(_)) => self.subscribe(ui),
            Some(ui_request_body::Body::Command(r)) => match r.validate_from_ui() {
                Ok(()) => self.route(ui, r, now),
                Err(e) => {
                    let mut fx = Effects::default();
                    fx.to_ui.push(Self::ui_error(
                        ui,
                        r.request_id,
                        ErrorKindCode::InvalidRequest,
                        &e.to_string(),
                    ));
                    fx
                }
            },
            Some(ui_request_body::Body::TerminalLease(lease)) => {
                self.terminal_lease(ui, &lease.terminal_id, now);
                Effects::default()
            }
            None => Effects::default(),
        }
    }
}
