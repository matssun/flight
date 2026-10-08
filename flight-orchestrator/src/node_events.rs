// SPDX-License-Identifier: MIT

use crate::node_image::NodeImage;
use crate::orchestrator_core::ui_result;
use crate::{ConnId, Effects, OrchestratorCore};
use flight_proto::{
    fleet_change::Change, node_body, orchestrator_body, response_result, Delta, ErrorInfo,
    ErrorKindCode, NodeFrame, OrchestratorFrame, Response, ResyncRequest, Snapshot, Step, Validate,
};
use flight_state::HostId;

fn to_node(body: orchestrator_body::Body) -> OrchestratorFrame {
    OrchestratorFrame { body: Some(body) }
}

impl OrchestratorCore {
    pub fn on_node_frame(&mut self, conn: ConnId, frame: NodeFrame, now: u64) -> Effects {
        self.clock = self.clock.max(now);
        let mut fx = Effects::default();
        if let Err(reject) = frame.validate() {
            self.violation(conn, format!("invalid frame: {reject}"), &mut fx);
            return fx;
        }
        let Some(state) = self.conns.get(&conn).cloned() else {
            fx.close.push((conn, "unknown connection".to_owned()));
            return fx;
        };
        let Some(body) = frame.body else {
            return fx;
        };
        match (state.hello_done, body) {
            (false, node_body::Body::Hello(hello)) => {
                self.on_hello(conn, &state.peer, hello, now, &mut fx)
            }
            (false, _) => self.violation(conn, "first frame must be a hello".into(), &mut fx),
            (true, node_body::Body::Hello(_)) => {
                self.violation(conn, "second hello on one connection".into(), &mut fx)
            }
            (true, body) => {
                self.touch(&state.peer, now, &mut fx);
                self.on_stream_frame(conn, &state.peer, body, &mut fx);
            }
        }
        fx
    }

    fn on_stream_frame(
        &mut self,
        conn: ConnId,
        node: &HostId,
        body: node_body::Body,
        fx: &mut Effects,
    ) {
        match body {
            node_body::Body::Snapshot(s) => self.on_snapshot(conn, node, &s, fx),
            node_body::Body::Delta(d) => self.on_delta(conn, node, &d, fx),
            node_body::Body::Response(r) => self.on_response(conn, r, fx),
            node_body::Body::Heartbeat(_) | node_body::Body::Hello(_) => {}
        }
    }

    fn on_snapshot(&mut self, conn: ConnId, node: &HostId, snapshot: &Snapshot, fx: &mut Effects) {
        let (image, incarnation) = match (
            NodeImage::from_snapshot(node, snapshot),
            snapshot.incarnation(),
        ) {
            (Ok(image), Ok(inc)) => (image, inc),
            (Err(e), _) | (_, Err(e)) => {
                return self.violation(conn, format!("bad snapshot: {e}"), fx)
            }
        };
        let Some(entry) = self.nodes.get_mut(node) else {
            return;
        };
        entry.cursor.on_snapshot(incarnation);
        entry.resync_requested = false;
        let (mut changes, republish) = entry.image.replace(node, image);
        if republish {
            changes.push(Change::NodeUpsert(entry.view(node)));
        }
        self.publish(fx, changes);
    }

    fn on_delta(&mut self, conn: ConnId, node: &HostId, delta: &Delta, fx: &mut Effects) {
        let Some(entry) = self.nodes.get_mut(node) else {
            return;
        };
        let incarnation = match delta.incarnation() {
            Ok(i) => i,
            Err(e) => return self.violation(conn, format!("bad delta: {e}"), fx),
        };
        if entry.cursor.on_delta(incarnation, delta.sequence) == Step::Resync {
            // Keep the last consistent image; ask once for a snapshot.
            if !entry.resync_requested {
                entry.resync_requested = true;
                fx.to_nodes.push((
                    conn,
                    to_node(orchestrator_body::Body::Resync(ResyncRequest {
                        reason: "delta out of sequence".to_owned(),
                    })),
                ));
            }
            return;
        }
        match entry.image.apply(node, delta) {
            Ok(changes) => self.publish(fx, changes),
            Err(e) => self.violation(conn, format!("bad delta: {e}"), fx),
        }
    }

    fn on_response(&mut self, conn: ConnId, response: Response, fx: &mut Effects) {
        let Some(req) = self.pending.complete(conn, response.request_id) else {
            return;
        };
        let result = response.result.unwrap_or_else(|| {
            response_result::Result::Error(ErrorInfo {
                kind: ErrorKindCode::InvalidRequest as i32,
                message: "empty response".to_owned(),
            })
        });
        let result = match req.terminal {
            Some(id) => self.terminal_answer(&id, result, fx),
            None => result,
        };
        fx.to_ui.push(ui_result(req.ui, req.ui_request_id, result));
    }
}
