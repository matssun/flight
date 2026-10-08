// SPDX-License-Identifier: MIT

use crate::{OpenedTerminal, SessionRequest, TerminalSpec};
use flight_proto::{
    node_body, response_result, ErrorInfo, ErrorKindCode, NodeFrame, Preview, Response,
};
use flight_state::{PaneId, ServerId};

/// A control request the node could not carry out, in wire terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlError {
    pub kind: ErrorKindCode,
    pub message: String,
}

impl ControlError {
    pub fn new(kind: ErrorKindCode, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn info(&self) -> ErrorInfo {
        ErrorInfo {
            kind: self.kind as i32,
            message: self.message.clone(),
        }
    }
}

/// What a node can do on request, against its own tmux servers. These calls perform
/// external I/O and may be slow: they run on a [`ControlJob`], never under the session lock.
pub trait Control: Send + Sync {
    /// The last `lines` lines of the pane's visible screen (plain text).
    fn capture(&self, server: &ServerId, pane: &PaneId, lines: u32)
        -> Result<String, ControlError>;

    /// Kill the pane, but only if it is still the process the request was issued against
    /// (`expected_pid`): a reused pane id must never be killed by a request meant for its
    /// predecessor.
    fn kill_pane(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError>;

    /// Make the pane the active pane of its window and its window the current one of its
    /// session, but only if it still is the process the request was about. Never touches a
    /// tmux client. A control that cannot do this refuses; there is no unguarded fallback.
    fn reveal_pane(
        &self,
        _server: &ServerId,
        _pane: &PaneId,
        _expected_pid: u32,
    ) -> Result<(), ControlError> {
        Err(ControlError::new(
            ErrorKindCode::Unsupported,
            "revealing a pane is not available on this control",
        ))
    }

    /// Start a terminal onto the pane: a tmux client in a PTY the node owns, attached only if
    /// the pane still is the process the request was about. Nothing is created otherwise.
    fn open_terminal(&self, _spec: &TerminalSpec) -> Result<OpenedTerminal, ControlError> {
        Err(ControlError::new(
            ErrorKindCode::Unsupported,
            "terminals are not available on this control",
        ))
    }

    /// Create a detached session running a program from a closed set. All or nothing: on
    /// failure no session of that name is left behind, and an existing one is never touched.
    fn create_session(&self, _request: &SessionRequest) -> Result<(), ControlError> {
        Err(ControlError::new(
            ErrorKindCode::Unsupported,
            "creating a session is not available on this control",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Preview {
        server: ServerId,
        pane: PaneId,
        lines: u32,
    },
    Kill {
        server: ServerId,
        pane: PaneId,
        pid: u32,
    },
    Reveal {
        server: ServerId,
        pane: PaneId,
        pid: u32,
    },
    Create(SessionRequest),
    OpenTerminal(TerminalSpec),
}

/// A validated control request, detached from the session: everything needed to perform it
/// is captured here, so it can run after the session lock is released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlJob {
    request_id: u64,
    op: Op,
}

impl ControlJob {
    pub(crate) fn preview(request_id: u64, server: ServerId, pane: PaneId, lines: u32) -> Self {
        Self {
            request_id,
            op: Op::Preview {
                server,
                pane,
                lines,
            },
        }
    }

    pub(crate) fn kill(request_id: u64, server: ServerId, pane: PaneId, pid: u32) -> Self {
        Self {
            request_id,
            op: Op::Kill { server, pane, pid },
        }
    }

    pub(crate) fn reveal(request_id: u64, server: ServerId, pane: PaneId, pid: u32) -> Self {
        Self {
            request_id,
            op: Op::Reveal { server, pane, pid },
        }
    }

    pub(crate) fn create(request_id: u64, request: SessionRequest) -> Self {
        Self {
            request_id,
            op: Op::Create(request),
        }
    }

    pub(crate) fn open_terminal(spec: TerminalSpec) -> Self {
        Self {
            request_id: spec.request_id,
            op: Op::OpenTerminal(spec),
        }
    }

    /// The terminal this job opens, if it is one. Opening hands back a live process, which a
    /// plain response cannot carry, so the caller runs it with [`Control::open_terminal`]
    /// and answers itself.
    pub fn terminal(&self) -> Option<&TerminalSpec> {
        match &self.op {
            Op::OpenTerminal(spec) => Some(spec),
            _ => None,
        }
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Perform the operation and build the response frame. Blocks on external I/O.
    pub fn execute(self, control: &dyn Control, now: u64) -> NodeFrame {
        let result = match &self.op {
            Op::Preview {
                server,
                pane,
                lines,
            } => control.capture(server, pane, *lines).map(|text| {
                response_result::Result::Preview(Preview {
                    text,
                    captured_at: now,
                })
            }),
            Op::Kill { server, pane, pid } => control
                .kill_pane(server, pane, *pid)
                .map(|()| response_result::Result::Done(response_result::Done {})),
            Op::Reveal { server, pane, pid } => control
                .reveal_pane(server, pane, *pid)
                .map(|()| response_result::Result::Done(response_result::Done {})),
            Op::Create(request) => control
                .create_session(request)
                .map(|()| response_result::Result::Done(response_result::Done {})),
            // A terminal is opened through `Control::open_terminal`, never through this path.
            Op::OpenTerminal(_) => Err(ControlError::new(
                ErrorKindCode::Unsupported,
                "a terminal cannot be opened as a plain request",
            )),
        };
        response_frame(self.request_id, result)
    }
}

pub(crate) fn response_frame(
    request_id: u64,
    result: Result<response_result::Result, ControlError>,
) -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Response(Response {
            request_id,
            result: Some(result.unwrap_or_else(|e| response_result::Result::Error(e.info()))),
        })),
    }
}

/// The plain success answer to a request.
pub fn done_frame(request_id: u64) -> NodeFrame {
    response_frame(
        request_id,
        Ok(response_result::Result::Done(response_result::Done {})),
    )
}

/// A failure response for a request that never reached a [`Control`] (busy, timed out).
pub fn error_frame(request_id: u64, kind: ErrorKindCode, message: &str) -> NodeFrame {
    response_frame(request_id, Err(ControlError::new(kind, message)))
}
