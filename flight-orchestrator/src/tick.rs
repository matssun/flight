// SPDX-License-Identifier: MIT

use crate::liveness::status_change;
use crate::{Effects, Liveness, OrchestratorCore};
use flight_proto::{orchestrator_body, ErrorKindCode, Heartbeat, OrchestratorFrame};

impl OrchestratorCore {
    /// Advance time: ping live nodes, mark silent ones `Stale`, drop long-silent ones, and
    /// fail requests that outlived their deadline. Pane state is never touched.
    pub fn tick(&mut self, now: u64) -> Effects {
        self.clock = self.clock.max(now);
        let mut fx = Effects::default();
        let (stale_after, disconnect_after) =
            (self.config.stale_after(), self.config.disconnect_after());
        let interval = u64::from(self.config.heartbeat_interval_secs);
        let mut changes = Vec::new();
        let mut dropped = Vec::new();
        let mut notes = Vec::new();
        for (id, entry) in &mut self.nodes {
            let Some(conn) = entry.conn else { continue };
            let silent = now.saturating_sub(entry.last_seen);
            if silent > disconnect_after {
                notes.push(format!(
                    "{}: heartbeat timeout, no frame for {silent}s (limit {disconnect_after}s)",
                    entry.display_name
                ));
                dropped.push(conn);
                continue;
            }
            if silent > stale_after && entry.liveness == Liveness::Connected {
                entry.liveness = Liveness::Stale;
                changes.push(status_change(id, Liveness::Stale));
                notes.push(format!(
                    "{}: Stale, no frame for {silent}s (stale after {stale_after}s; heartbeat every {interval}s)",
                    entry.display_name
                ));
            }
            if now.saturating_sub(entry.last_ping) >= interval {
                entry.last_ping = now;
                fx.to_nodes.push((
                    conn,
                    OrchestratorFrame {
                        body: Some(orchestrator_body::Body::Heartbeat(Heartbeat { seq: now })),
                    },
                ));
            }
        }
        fx.notes.extend(notes);
        self.publish(&mut fx, changes);
        for conn in dropped {
            fx.close.push((conn, "heartbeat timeout".to_owned()));
            self.drop_conn(conn, &mut fx);
        }
        for req in self.pending.expired(now) {
            fx.to_ui.push(Self::ui_error(
                req.ui,
                req.ui_request_id,
                ErrorKindCode::NodeUnreachable,
                "request timed out",
            ));
        }
        fx
    }
}
