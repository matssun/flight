// SPDX-License-Identifier: MIT

use crate::UiId;
use flight_proto::{
    fleet_change::Change, ui_event_body, FleetDelta, FleetSnapshot, Incarnation, UiEvent,
};
use std::collections::BTreeMap;

/// Fan-out of fleet changes to subscribed UIs. Each subscriber has its own sequence, restarted
/// by its snapshot, under the orchestrator's incarnation, so deltas from before an
/// orchestrator restart can never continue a stream.
pub(crate) struct FleetHub {
    incarnation: Incarnation,
    next_sequence: BTreeMap<UiId, u64>,
}

impl FleetHub {
    pub(crate) fn new(incarnation: Incarnation) -> Self {
        Self {
            incarnation,
            next_sequence: BTreeMap::new(),
        }
    }

    pub(crate) fn incarnation(&self) -> Incarnation {
        self.incarnation
    }

    pub(crate) fn subscribe(&mut self, ui: UiId, snapshot: FleetSnapshot) -> UiEvent {
        self.next_sequence.insert(ui, 1);
        UiEvent {
            body: Some(ui_event_body::Body::Snapshot(snapshot)),
        }
    }

    pub(crate) fn unsubscribe(&mut self, ui: UiId) {
        self.next_sequence.remove(&ui);
    }

    pub(crate) fn publish(&mut self, changes: &[Change]) -> Vec<(UiId, UiEvent)> {
        let mut out = Vec::new();
        for (ui, next) in &mut self.next_sequence {
            for change in changes {
                out.push((
                    *ui,
                    UiEvent {
                        body: Some(ui_event_body::Body::Delta(FleetDelta {
                            incarnation: self.incarnation.as_bytes().to_vec(),
                            sequence: *next,
                            change: Some(change.clone()),
                        })),
                    },
                ));
                *next = next.saturating_add(1);
            }
        }
        out
    }
}
