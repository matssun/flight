// SPDX-License-Identifier: MIT

use crate::conn_state::ConnState;
use crate::fleet_hub::FleetHub;
use crate::node_entry::NodeEntry;
use crate::pending::Pending;
use crate::{ConnId, Effects, OrchestratorConfig, UiId};
use flight_proto::{
    fleet_change::Change, response_result, ui_event_body, ErrorInfo, ErrorKindCode, FleetSnapshot,
    Incarnation, Response, UiEvent,
};
use flight_state::HostId;
use std::collections::{BTreeMap, HashMap};

/// The live control plane as a state machine. Events in (`node_connected`, `on_node_frame`,
/// `node_disconnected`, `tick`, `subscribe`, `ui_request`), [`Effects`] out. Behaviour is split
/// across `node_events`, `tick`, `ui_events` and `routing`.
pub struct OrchestratorCore {
    pub(crate) config: OrchestratorConfig,
    pub(crate) nodes: BTreeMap<HostId, NodeEntry>,
    pub(crate) conns: HashMap<ConnId, ConnState>,
    pub(crate) hub: FleetHub,
    pub(crate) pending: Pending,
    /// The latest time any event carried: ages in operator notes are measured against it.
    pub(crate) clock: u64,
}

impl OrchestratorCore {
    /// `incarnation` identifies this orchestrator process (fresh and random per run).
    pub fn new(config: OrchestratorConfig, incarnation: Incarnation) -> Self {
        Self {
            config,
            nodes: BTreeMap::new(),
            conns: HashMap::new(),
            hub: FleetHub::new(incarnation),
            pending: Pending::default(),
            clock: 0,
        }
    }

    pub fn incarnation(&self) -> Incarnation {
        self.hub.incarnation()
    }

    /// The whole fleet as it is now: authoritative on its own.
    pub fn fleet_snapshot(&self) -> FleetSnapshot {
        FleetSnapshot {
            incarnation: self.hub.incarnation().as_bytes().to_vec(),
            nodes: self.nodes.iter().map(|(id, e)| e.view(id)).collect(),
        }
    }

    /// Publish fleet changes to every subscriber.
    pub(crate) fn publish(&mut self, fx: &mut Effects, changes: Vec<Change>) {
        if !changes.is_empty() {
            fx.to_ui.extend(self.hub.publish(&changes));
        }
    }

    /// An error answer to a UI request.
    pub(crate) fn ui_error(
        ui: UiId,
        request_id: u64,
        kind: ErrorKindCode,
        message: &str,
    ) -> (UiId, UiEvent) {
        ui_result(
            ui,
            request_id,
            response_result::Result::Error(ErrorInfo {
                kind: kind as i32,
                message: message.to_owned(),
            }),
        )
    }
}

pub(crate) fn ui_result(
    ui: UiId,
    request_id: u64,
    result: response_result::Result,
) -> (UiId, UiEvent) {
    (
        ui,
        UiEvent {
            body: Some(ui_event_body::Body::Response(Response {
                request_id,
                result: Some(result),
            })),
        },
    )
}
