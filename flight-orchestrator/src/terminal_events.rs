// SPDX-License-Identifier: MIT

use crate::terminals::{Phase, Side, Terminal, TerminalId};
use crate::{ConnId, Effects, OrchestratorCore, UiId};
use flight_proto::{
    command_kind::Kind, response_result, ErrorInfo, ErrorKindCode, ExitReasonCode, PaneRefMsg,
    Request, TerminalOpened, TERMINAL_ID_LEN,
};
use flight_state::HostId;

/// Why a terminal stream was not accepted. Deliberately one answer: an unknown, expired,
/// reused or foreign id must look the same to the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachRefused;

/// Whether the other end is already there (so the relay can start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attached {
    pub both: bool,
}

fn pane_key(pane: &Option<PaneRefMsg>) -> String {
    pane.as_ref()
        .map(|p| format!("{}/{}/{}", p.host, p.server, p.pane))
        .unwrap_or_default()
}

impl OrchestratorCore {
    /// The authenticated identity behind a UI connection (the transport knows it; tests may
    /// not say, in which case the connection stands for itself).
    pub fn ui_identified(&mut self, ui: UiId, identity: &str) {
        self.ui_identity.insert(ui, identity.to_owned());
    }

    fn identity_of(&self, ui: UiId) -> String {
        self.ui_identity
            .get(&ui)
            .cloned()
            .unwrap_or_else(|| format!("ui-connection-{}", ui.0))
    }

    /// Admission for an `OpenTerminal` being routed: limits, then a minted id bound to the
    /// asking identity, the node connection, the pane and the pid. Returns the id and the
    /// request to forward (the same command with the id filled in), or the refusal.
    pub(crate) fn admit_terminal(
        &mut self,
        ui: UiId,
        host: &HostId,
        conn: ConnId,
        request: &Request,
        now: u64,
        fx: &mut Effects,
    ) -> Result<(TerminalId, Request), (ErrorKindCode, &'static str)> {
        let Some(Kind::OpenTerminal(open)) = request.command.as_ref().and_then(|c| c.kind.as_ref())
        else {
            return Err((ErrorKindCode::InvalidRequest, "not a terminal request"));
        };
        let identity = self.identity_of(ui);
        let pane = pane_key(&open.pane_ref);
        // One terminal per (UI, pane): opening again replaces the earlier one.
        for old in self
            .terminals
            .ids_where(|t| t.ui_identity == identity && t.pane == pane)
        {
            self.terminals.remove(&old);
            fx.terminals_ended.push((old, ExitReasonCode::ClosedByUi));
        }
        let limits = self.config.terminal_limits;
        if self.terminals.len() >= limits.total
            || self.terminals.count_where(|t| &t.host == host) >= limits.per_node
            || self.terminals.count_where(|t| t.ui_identity == identity) >= limits.per_ui
        {
            return Err((ErrorKindCode::Busy, "too many open terminals"));
        }
        let Some(id) = (self.terminal_ids)() else {
            return Err((ErrorKindCode::NodeUnreachable, "cannot mint a terminal id"));
        };
        self.terminals.insert(
            id,
            Terminal {
                ui_identity: identity,
                host: host.clone(),
                conn,
                pane,
                pid: open.expected_pid,
                phase: Phase::Opening,
                deadline: now.saturating_add(self.config.request_timeout_secs),
                ui_attached: false,
                node_attached: false,
            },
        );
        let mut forwarded = request.clone();
        if let Some(Kind::OpenTerminal(o)) =
            forwarded.command.as_mut().and_then(|c| c.kind.as_mut())
        {
            o.terminal_id = id.to_vec();
        }
        Ok((id, forwarded))
    }

    /// Turn the node's answer to an `OpenTerminal` into what the UI is told.
    pub(crate) fn terminal_answer(
        &mut self,
        id: &TerminalId,
        result: response_result::Result,
        fx: &mut Effects,
    ) -> response_result::Result {
        let refuse = |kind: ErrorKindCode, message: &str| {
            response_result::Result::Error(ErrorInfo {
                kind: kind as i32,
                message: message.to_owned(),
            })
        };
        match result {
            response_result::Result::Done(_) if self.terminal_opened(id, self.clock) => {
                if let Some(t) = self.terminals.get(id) {
                    fx.notes.push(format!(
                        "terminal opened: ui {} node {} pane {} pid {}",
                        t.ui_identity, t.host, t.pane, t.pid
                    ));
                }
                response_result::Result::Terminal(TerminalOpened {
                    terminal_id: id.to_vec(),
                })
            }
            response_result::Result::Error(e) => {
                self.terminal_open_failed(id);
                response_result::Result::Error(e)
            }
            _ => {
                // Done for a terminal that was replaced or ended meanwhile, or a payload that
                // is not an answer to an open.
                self.terminal_open_failed(id);
                refuse(
                    ErrorKindCode::NodeUnreachable,
                    "the terminal is no longer wanted",
                )
            }
        }
    }

    /// The node answered an `OpenTerminal`: both ends now have the attach window.
    pub(crate) fn terminal_opened(&mut self, id: &TerminalId, now: u64) -> bool {
        let window = self.config.terminal_attach_window_secs;
        match self.terminals.get_mut(id) {
            Some(t) if t.phase == Phase::Opening => {
                t.phase = Phase::Opened;
                t.deadline = now.saturating_add(window);
                true
            }
            _ => false,
        }
    }

    /// The open failed or was refused: the id dies with no stream ever created.
    pub(crate) fn terminal_open_failed(&mut self, id: &TerminalId) {
        self.terminals.remove(id);
    }

    /// A stream arrived for `id` from `side`, authenticated as `identity` (the node's
    /// `HostId` string or the UI's fingerprint). Single use per side.
    pub fn terminal_attach(
        &mut self,
        side: Side,
        id: &[u8],
        identity: &str,
    ) -> Result<(TerminalId, Attached), AttachRefused> {
        let id: TerminalId = <[u8; TERMINAL_ID_LEN]>::try_from(id).map_err(|_| AttachRefused)?;
        let t = self.terminals.get_mut(&id).ok_or(AttachRefused)?;
        match side {
            Side::Node => {
                // The node may attach as soon as it started the PTY, which can be before
                // its answer was processed on the other connection.
                if t.node_attached || t.host.as_str() != identity {
                    return Err(AttachRefused);
                }
                t.node_attached = true;
            }
            Side::Ui => {
                if t.phase != Phase::Opened || t.ui_attached || t.ui_identity != identity {
                    return Err(AttachRefused);
                }
                t.ui_attached = true;
            }
        }
        let both = t.ui_attached && t.node_attached;
        if both {
            // From here only an end of either stream removes it; the deadline no longer applies.
            t.deadline = u64::MAX;
        }
        Ok((id, Attached { both }))
    }

    /// A terminal's stream ended (either side, any reason). The id is dead afterwards.
    pub fn terminal_ended(&mut self, id: &TerminalId) {
        self.terminals.remove(id);
    }

    /// Whether `id` is still a live terminal.
    pub fn terminal_is_open(&self, id: &TerminalId) -> bool {
        self.terminals.get(id).is_some()
    }

    pub fn terminal_count(&self) -> usize {
        self.terminals.len()
    }

    /// End every terminal bound to a UI identity (it was revoked).
    pub fn end_ui_terminals(&mut self, identity: &str, reason: ExitReasonCode) -> Effects {
        let mut fx = Effects::default();
        for id in self.terminals.ids_where(|t| t.ui_identity == identity) {
            self.terminals.remove(&id);
            fx.terminals_ended.push((id, reason));
        }
        fx
    }

    /// End every terminal on a node connection that is going away.
    pub(crate) fn end_conn_terminals(
        &mut self,
        conn: ConnId,
        reason: ExitReasonCode,
        fx: &mut Effects,
    ) {
        for id in self.terminals.ids_where(|t| t.conn == conn) {
            self.terminals.remove(&id);
            fx.terminals_ended.push((id, reason));
        }
    }

    /// Terminals that never got both ends in time die; so does a request nobody answered.
    pub(crate) fn expire_terminals(&mut self, now: u64, fx: &mut Effects) {
        for id in self.terminals.ids_where(|t| t.deadline <= now) {
            self.terminals.remove(&id);
            fx.terminals_ended.push((id, ExitReasonCode::StartFailed));
        }
    }
}
