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
        Kind::CreateSurface(_) => capability::CREATE_SURFACE,
        // A fresh start must reach a node that knows the difference from a restore: one that
        // does not would restore (and, in time, resume), which is not what was asked.
        Kind::SavedAction(c) if c.action == flight_proto::SavedActionCode::RestoreFresh as i32 => {
            capability::AGENT_RESUME
        }
        Kind::SavedAction(_) => capability::SAVED_ACTIONS,
    }
}

impl OrchestratorCore {
    /// The one host whose published panes include a surface of this workspace. A workspace id
    /// that no host has, or that two hosts claim, names no host: nothing is routed on a guess.
    fn host_of_workspace(&self, workspace_id: &str) -> Option<HostId> {
        let mut hosts = self.nodes.iter().filter(|(_, e)| {
            e.image
                .panes
                .values()
                .any(|p| p.workspace_id == workspace_id)
        });
        let (host, _) = hosts.next()?;
        hosts.next().is_none().then(|| host.clone())
    }

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
        // A surface belongs to a workspace and so to the workspace's host: the caller names the
        // workspace and nothing else, and this side finds the host. A pane-addressed command
        // names its host in the pane.
        let target = match command.kind.as_ref() {
            Some(Kind::CreateSurface(c)) => match self.host_of_workspace(&c.workspace_id) {
                Some(host) => host,
                None => {
                    fail(
                        &mut fx,
                        ErrorKindCode::UnknownWorkspace,
                        "no such workspace",
                    );
                    return fx;
                }
            },
            _ => {
                let Some(target) = command.target_host().map(HostId::new) else {
                    fail(&mut fx, ErrorKindCode::InvalidRequest, "no target");
                    return fx;
                };
                target
            }
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
