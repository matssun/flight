// SPDX-License-Identifier: MIT

use crate::pending::PendingRequest;
use crate::{Effects, Liveness, OrchestratorCore, UiId};
use flight_proto::{
    capability, command_kind::Kind, orchestrator_body, ErrorKindCode, OrchestratorFrame, Request,
};
use flight_state::HostId;

fn required_capability(kind: &Kind) -> &'static str {
    match kind {
        Kind::GetPreview(_) => capability::PREVIEW,
        Kind::RevealPane(_) => capability::GUARDED_REVEAL,
        Kind::OpenTerminal(_) => capability::TERMINAL,
        Kind::SendInput(_) => capability::SEND_INPUT,
        Kind::KillPane(_) => capability::KILL,
        Kind::CreateSession(_) => capability::CREATE_SESSION,
    }
}

impl OrchestratorCore {
    /// Route a validated command to its node by stable id. Only a currently connected node
    /// receives it; otherwise it fails at once. Commands are never queued: their target may
    /// no longer mean what it meant when the user issued them.
    pub(crate) fn route(&mut self, ui: UiId, request: Request, now: u64) -> Effects {
        let mut fx = Effects::default();
        let request_id = request.request_id;
        let fail = |fx: &mut Effects, kind, msg: &str| {
            fx.to_ui.push(Self::ui_error(ui, request_id, kind, msg));
        };
        let Some(command) = request.command.as_ref() else {
            fail(&mut fx, ErrorKindCode::InvalidRequest, "no command");
            return fx;
        };
        let Some(target) = command.target_host().map(HostId::new) else {
            fail(&mut fx, ErrorKindCode::InvalidRequest, "no target");
            return fx;
        };
        let (Some(entry), Some(kind)) = (self.nodes.get(&target), command.kind.as_ref()) else {
            fail(&mut fx, ErrorKindCode::InvalidRequest, "unknown node");
            return fx;
        };
        let conn = match (entry.liveness, entry.conn) {
            (Liveness::Connected, Some(conn)) => conn,
            _ => {
                fail(
                    &mut fx,
                    ErrorKindCode::NodeUnreachable,
                    "node is not connected",
                );
                return fx;
            }
        };
        if !entry
            .accepted
            .iter()
            .any(|c| c == required_capability(kind))
        {
            fail(
                &mut fx,
                ErrorKindCode::Unsupported,
                "node does not offer this operation",
            );
            return fx;
        }
        // A terminal gets its identity here: the id is minted by this side, bound to the
        // asking UI, this node connection, the pane and the pid, and written into the
        // command the node sees. Nothing else is touched.
        let mut terminal = None;
        let opens_terminal = matches!(kind, Kind::OpenTerminal(_));
        let mut request = request;
        if opens_terminal {
            match self.admit_terminal(ui, &target, conn, &request, now, &mut fx) {
                Ok((id, forwarded)) => {
                    terminal = Some(id);
                    request = forwarded;
                }
                Err((kind, message)) => {
                    fail(&mut fx, kind, message);
                    return fx;
                }
            }
        }
        let node_request_id = self.pending.add(PendingRequest {
            conn,
            ui,
            ui_request_id: request.request_id,
            deadline: now.saturating_add(self.config.request_timeout_secs),
            terminal,
        });
        fx.to_nodes.push((
            conn,
            OrchestratorFrame {
                body: Some(orchestrator_body::Body::Request(Request {
                    request_id: node_request_id,
                    command: request.command,
                })),
            },
        ));
        fx
    }
}
