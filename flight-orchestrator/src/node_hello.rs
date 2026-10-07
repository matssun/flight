// SPDX-License-Identifier: MIT

use crate::liveness::status_change;
use crate::node_entry::NodeEntry;
use crate::{ConnId, Effects, Liveness, OrchestratorCore};
use flight_proto::{
    capability, fleet_change::Change, orchestrator_body, ErrorKindCode, Goodbye, NodeHello,
    OrchestratorFrame, OrchestratorHello, CURRENT_VERSION,
};
use flight_state::HostId;

fn to_node(body: orchestrator_body::Body) -> OrchestratorFrame {
    OrchestratorFrame { body: Some(body) }
}

impl OrchestratorCore {
    pub(crate) fn on_hello(
        &mut self,
        conn: ConnId,
        peer: &HostId,
        hello: NodeHello,
        now: u64,
        fx: &mut Effects,
    ) {
        if hello.node_id != peer.as_str() {
            return self.violation(
                conn,
                "hello node_id differs from the authenticated peer".into(),
                fx,
            );
        }
        let theirs = hello.version.unwrap_or_default();
        let agreed = match CURRENT_VERSION.negotiate(theirs) {
            Ok(v) => v,
            Err(reject) => {
                fx.to_nodes.push((
                    conn,
                    to_node(orchestrator_body::Body::Goodbye(Goodbye {
                        reason: ErrorKindCode::ProtocolMismatch as i32,
                        message: reject.to_string(),
                    })),
                ));
                return self.violation(conn, reject.to_string(), fx);
            }
        };
        let accepted = capability::negotiate(&hello.capabilities, &capability::KNOWN);
        if let Some(state) = self.conns.get_mut(&conn) {
            state.hello_done = true;
        }
        let changes = self.bind(conn, peer, hello.display_name, accepted.clone(), now, fx);
        self.publish(fx, changes);
        fx.to_nodes.push((
            conn,
            to_node(orchestrator_body::Body::Hello(OrchestratorHello {
                version: Some(agreed),
                accepted_capabilities: accepted,
                heartbeat_interval_secs: self.config.heartbeat_interval_secs,
            })),
        ));
    }

    fn bind(
        &mut self,
        conn: ConnId,
        peer: &HostId,
        name: String,
        accepted: Vec<String>,
        now: u64,
        fx: &mut Effects,
    ) -> Vec<Change> {
        let Some(entry) = self.nodes.get_mut(peer) else {
            let entry = NodeEntry::new(name, accepted, conn, now);
            let view = entry.view(peer);
            self.nodes.insert(peer.clone(), entry);
            return vec![Change::NodeUpsert(view)];
        };
        let superseded = entry.conn.filter(|old| *old != conn);
        let was = entry.liveness;
        let renamed = entry.display_name != name;
        entry.display_name = name;
        entry.accepted = accepted;
        entry.conn = Some(conn);
        entry.liveness = Liveness::Connected;
        entry.last_seen = now;
        entry.last_ping = now;
        // Whatever the stream held, it ends here: the next accepted state is a snapshot.
        entry.cursor.reset();
        entry.resync_requested = false;
        let change = if renamed {
            Change::NodeUpsert(entry.view(peer))
        } else if was != Liveness::Connected {
            status_change(peer, Liveness::Connected)
        } else {
            return self.supersede(superseded, fx);
        };
        let mut changes = vec![change];
        changes.extend(self.supersede(superseded, fx));
        changes
    }

    fn supersede(&mut self, old: Option<ConnId>, fx: &mut Effects) -> Vec<Change> {
        if let Some(old) = old {
            self.conns.remove(&old);
            self.fail_pending(old, fx);
            fx.close
                .push((old, "superseded by a newer connection".to_owned()));
        }
        Vec::new()
    }
}
