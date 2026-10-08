// SPDX-License-Identifier: MIT

use crate::control::response_frame;
use crate::{ControlError, ControlJob, NodeCore, Round, TerminalSpec};
use flight_proto::{
    capability, command_kind::Kind, node_body, orchestrator_body, Command, ErrorKindCode,
    Heartbeat, NodeFrame, NodeHello, OrchestratorFrame, PaneRefMsg, ProtocolVersion, Request,
    Validate, CURRENT_VERSION, TERMINAL_ID_LEN,
};
use flight_state::PaneRef;

/// What this node can do. `send_input` is not offered: there is no input path yet. A node
/// never switches a client (it has none): it reveals a pane in its own tmux hierarchy.
pub const ADVERTISED_CAPABILITIES: [&str; 5] = [
    capability::PREVIEW,
    capability::GUARDED_REVEAL,
    capability::TERMINAL,
    capability::KILL,
    capability::CREATE_SESSION,
];

/// Frames to send, control work to run, and whether the node should close the stream.
#[derive(Debug, Default, PartialEq)]
pub struct SessionOutput {
    pub frames: Vec<NodeFrame>,
    /// Validated control requests. The session never performs them: the caller runs each one
    /// ([`ControlJob::execute`]) off the session lock and sends the response frame.
    pub jobs: Vec<ControlJob>,
    /// Set when the peer is unusable (e.g. protocol major mismatch); the reason is for logs.
    pub close: Option<String>,
}

impl SessionOutput {
    fn frames(frames: Vec<NodeFrame>) -> Self {
        Self {
            frames,
            ..Self::default()
        }
    }

    fn job(job: ControlJob) -> Self {
        Self {
            jobs: vec![job],
            ..Self::default()
        }
    }

    fn close(reason: String) -> Self {
        Self {
            close: Some(reason),
            ..Self::default()
        }
    }
}

/// The node's side of one orchestrator stream, as a pure state machine over frames: no
/// sockets, so it runs identically over an in-memory pipe and, later, a gRPC stream.
pub struct NodeSession {
    core: NodeCore,
    display_name: String,
    /// `Some` once the orchestrator's hello was accepted: the capabilities it accepted.
    accepted: Option<Vec<String>>,
    /// What this node offers: everything this build can do, less what the operator switched off.
    offered: Vec<&'static str>,
}

fn frame(body: node_body::Body) -> NodeFrame {
    NodeFrame { body: Some(body) }
}

impl NodeSession {
    pub fn new(core: NodeCore, display_name: impl Into<String>) -> Self {
        Self {
            core,
            display_name: display_name.into(),
            accepted: None,
            offered: ADVERTISED_CAPABILITIES.to_vec(),
        }
    }

    /// Do not offer interactive terminals (`--no-terminal`): a read-only fleet.
    pub fn without_terminal(mut self) -> Self {
        self.offered.retain(|c| *c != capability::TERMINAL);
        self
    }

    pub fn core(&self) -> &NodeCore {
        &self.core
    }

    /// The first frame on a (re)connected stream.
    pub fn connect(&mut self, servers: Vec<String>) -> NodeFrame {
        self.accepted = None;
        frame(node_body::Body::Hello(NodeHello {
            version: Some(CURRENT_VERSION),
            node_id: self.core.host().as_str().to_owned(),
            display_name: self.display_name.clone(),
            capabilities: self.offered.iter().map(|c| (*c).to_owned()).collect(),
            servers,
        }))
    }

    pub fn heartbeat(&self, seq: u64) -> NodeFrame {
        frame(node_body::Body::Heartbeat(Heartbeat { seq }))
    }

    /// Fold observation rounds into the state. Before the handshake completes nothing is
    /// emitted: the snapshot sent afterwards already contains the change.
    pub fn observe(&mut self, rounds: Vec<Round>) -> Vec<NodeFrame> {
        let deltas: Vec<_> = rounds
            .into_iter()
            .flat_map(|r| self.core.apply(r))
            .collect();
        if self.accepted.is_none() {
            return Vec::new();
        }
        deltas
            .into_iter()
            .map(|d| frame(node_body::Body::Delta(d)))
            .collect()
    }

    pub fn on_frame(&mut self, incoming: OrchestratorFrame, _now: u64) -> SessionOutput {
        if let Err(reject) = incoming.validate() {
            return SessionOutput::close(format!("invalid frame from orchestrator: {reject}"));
        }
        match incoming.body {
            Some(orchestrator_body::Body::Hello(h)) => self.on_hello(h),
            Some(orchestrator_body::Body::Heartbeat(h)) => {
                SessionOutput::frames(vec![self.heartbeat(h.seq)])
            }
            Some(orchestrator_body::Body::Resync(_)) => {
                SessionOutput::frames(vec![self.snapshot_frame()])
            }
            Some(orchestrator_body::Body::Request(r)) => self.on_request(r),
            Some(orchestrator_body::Body::Goodbye(g)) => {
                SessionOutput::close(format!("orchestrator closed the stream: {}", g.message))
            }
            None => SessionOutput::default(),
        }
    }

    /// The complete current state as a frame; restarts the delta sequence. Used for a resync
    /// and when a backlog of deltas was discarded.
    pub fn snapshot_frame(&mut self) -> NodeFrame {
        frame(node_body::Body::Snapshot(self.core.snapshot()))
    }

    fn on_hello(&mut self, hello: flight_proto::OrchestratorHello) -> SessionOutput {
        let theirs = hello
            .version
            .unwrap_or(ProtocolVersion { major: 0, minor: 0 });
        if let Err(reject) = CURRENT_VERSION.negotiate(theirs) {
            return SessionOutput::close(reject.to_string());
        }
        self.accepted = Some(capability::negotiate(
            &hello.accepted_capabilities,
            &self.offered,
        ));
        SessionOutput::frames(vec![self.snapshot_frame()])
    }

    /// Validate a request under the session's state and turn it into a job; the caller runs
    /// the job without holding the session. Refusals are answered immediately.
    fn on_request(&self, request: Request) -> SessionOutput {
        let id = request.request_id;
        let planned = match request.command {
            Some(command) => self.plan(id, &command),
            None => Err(ControlError::new(
                ErrorKindCode::InvalidRequest,
                "no command",
            )),
        };
        match planned {
            Ok(job) => SessionOutput::job(job),
            Err(e) => SessionOutput::frames(vec![response_frame(id, Err(e))]),
        }
    }

    fn plan(&self, id: u64, command: &Command) -> Result<ControlJob, ControlError> {
        let accepted = self.accepted.as_ref().ok_or_else(|| {
            ControlError::new(ErrorKindCode::NotAuthorized, "handshake not complete")
        })?;
        let need = |cap: &str| {
            if accepted.iter().any(|a| a == cap) {
                Ok(())
            } else {
                Err(ControlError::new(
                    ErrorKindCode::Unsupported,
                    format!("capability {cap} not available"),
                ))
            }
        };
        match command.kind.as_ref() {
            Some(Kind::GetPreview(c)) => {
                need(capability::PREVIEW)?;
                let (pane, _pid) = self.own_pane(c.pane_ref.as_ref())?;
                Ok(ControlJob::preview(id, pane.server, pane.pane, c.lines))
            }
            Some(Kind::KillPane(c)) => {
                need(capability::KILL)?;
                let (pane, pid) = self.own_pane(c.pane_ref.as_ref())?;
                Ok(ControlJob::kill(id, pane.server, pane.pane, pid))
            }
            Some(Kind::CreateSession(c)) => {
                need(capability::CREATE_SESSION)?;
                if c.host != self.core.host().as_str() {
                    return Err(ControlError::new(
                        ErrorKindCode::InvalidRequest,
                        "wrong host",
                    ));
                }
                Ok(ControlJob::create(
                    id,
                    flight_state::ServerId::new(c.server.as_str()),
                    c.name.clone(),
                    c.dir.clone(),
                    c.command.clone(),
                ))
            }
            Some(Kind::RevealPane(c)) => {
                need(capability::GUARDED_REVEAL)?;
                let (pane, published) = self.own_pane(c.pane_ref.as_ref())?;
                // The caller was looking at one process; the node publishes another.
                if published != c.expected_pid {
                    return Err(ControlError::new(
                        ErrorKindCode::PaneChanged,
                        format!(
                            "pane {} is no longer the process you were looking at",
                            pane.pane
                        ),
                    ));
                }
                Ok(ControlJob::reveal(id, pane.server, pane.pane, published))
            }
            Some(Kind::OpenTerminal(c)) => {
                need(capability::TERMINAL)?;
                let (pane, published) = self.own_pane(c.pane_ref.as_ref())?;
                if published != c.expected_pid {
                    return Err(ControlError::new(
                        ErrorKindCode::PaneChanged,
                        format!(
                            "pane {} is no longer the process you were looking at",
                            pane.pane
                        ),
                    ));
                }
                let invalid =
                    |m: &str| ControlError::new(ErrorKindCode::InvalidRequest, m.to_owned());
                let terminal_id = <[u8; TERMINAL_ID_LEN]>::try_from(c.terminal_id.as_slice())
                    .map_err(|_| invalid("no terminal id"))?;
                let dim = |v: u32| u16::try_from(v).map_err(|_| invalid("terminal size"));
                Ok(ControlJob::open_terminal(TerminalSpec {
                    request_id: id,
                    terminal_id,
                    server: pane.server,
                    pane: pane.pane,
                    pid: published,
                    cols: dim(c.cols)?,
                    rows: dim(c.rows)?,
                    term: c.term.clone(),
                }))
            }
            Some(Kind::SendInput(_)) => Err(ControlError::new(
                ErrorKindCode::Unsupported,
                "input is not available yet",
            )),
            None => Err(ControlError::new(
                ErrorKindCode::InvalidRequest,
                "no command kind",
            )),
        }
    }

    /// The pane named by a request, only if it is an agent pane this node publishes, with the
    /// pid of the process published for it (the pane incarnation the request is about).
    fn own_pane(&self, msg: Option<&PaneRefMsg>) -> Result<(PaneRef, u32), ControlError> {
        let invalid = |m: &str| ControlError::new(ErrorKindCode::InvalidRequest, m.to_owned());
        let pane = PaneRef::try_from(msg.ok_or_else(|| invalid("no pane"))?)
            .map_err(|e| invalid(&e.to_string()))?;
        if pane.host != *self.core.host() {
            return Err(invalid("pane belongs to another host"));
        }
        let pid = self.core.pane_pid(&pane).ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::UnknownPane,
                format!("no agent pane {}", pane.pane),
            )
        })?;
        Ok((pane, pid))
    }
}
