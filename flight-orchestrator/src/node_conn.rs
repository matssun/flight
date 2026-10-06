// SPDX-License-Identifier: MIT

use crate::conn_state::ConnState;
use crate::liveness::status_change;
use crate::{ConnId, Effects, Liveness, OrchestratorCore};
use flight_proto::ErrorKindCode;
use flight_state::HostId;

impl OrchestratorCore {
    pub fn node_connected(&mut self, conn: ConnId, peer: HostId) {
        self.conns.insert(
            conn,
            ConnState {
                peer,
                hello_done: false,
            },
        );
    }

    pub fn node_disconnected(&mut self, conn: ConnId) -> Effects {
        let mut fx = Effects::default();
        self.drop_conn(conn, &mut fx);
        fx
    }

    pub(crate) fn touch(&mut self, node: &HostId, now: u64, fx: &mut Effects) {
        let Some(entry) = self.nodes.get_mut(node) else {
            return;
        };
        entry.last_seen = now;
        if entry.liveness == Liveness::Stale {
            entry.liveness = Liveness::Connected;
            self.publish(fx, vec![status_change(node, Liveness::Connected)]);
        }
    }

    pub(crate) fn violation(&mut self, conn: ConnId, reason: String, fx: &mut Effects) {
        fx.close.push((conn, reason));
        self.drop_conn(conn, fx);
    }

    pub(crate) fn drop_conn(&mut self, conn: ConnId, fx: &mut Effects) {
        let Some(state) = self.conns.remove(&conn) else {
            return;
        };
        self.fail_pending(conn, fx);
        let Some(entry) = self.nodes.get_mut(&state.peer) else {
            return;
        };
        if entry.conn != Some(conn) {
            return;
        }
        entry.conn = None;
        entry.cursor.reset();
        entry.resync_requested = false;
        if entry.liveness != Liveness::Disconnected {
            entry.liveness = Liveness::Disconnected;
            self.publish(fx, vec![status_change(&state.peer, Liveness::Disconnected)]);
        }
    }

    pub(crate) fn fail_pending(&mut self, conn: ConnId, fx: &mut Effects) {
        for req in self.pending.fail_conn(conn) {
            fx.to_ui.push(Self::ui_error(
                req.ui,
                req.ui_request_id,
                ErrorKindCode::NodeUnreachable,
                "node disconnected",
            ));
        }
    }
}
