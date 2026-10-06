// SPDX-License-Identifier: MIT

use crate::{Control, ControlError, NodeCore, Round};
use flight_proto::{
    capability, command_kind::Kind, node_body, orchestrator_body, response_result, Command,
    ErrorKindCode, Heartbeat, NodeFrame, NodeHello, OrchestratorFrame, PaneRefMsg, Preview,
    ProtocolVersion, Request, Response, Validate, CURRENT_VERSION,
};
use flight_state::PaneRef;

/// What this node can do. `send_input` and `switch` are not offered: the node has no client
/// to switch and no input path yet.
pub const ADVERTISED_CAPABILITIES: [&str; 3] = [
    capability::PREVIEW,
    capability::KILL,
    capability::CREATE_SESSION,
];

/// Frames to send, and whether the node should close the stream.
#[derive(Debug, Default, PartialEq)]
pub struct SessionOutput {
    pub frames: Vec<NodeFrame>,
    /// Set when the peer is unusable (e.g. protocol major mismatch); the reason is for logs.
    pub close: Option<String>,
}

impl SessionOutput {
    fn frames(frames: Vec<NodeFrame>) -> Self {
        Self {
            frames,
            close: None,
        }
    }
}

/// The node's side of one orchestrator stream, as a pure state machine over frames: no
/// sockets, so it runs identically over an in-memory pipe and, later, a gRPC stream.
pub struct NodeSession<C: Control> {
    core: NodeCore,
    control: C,
    display_name: String,
    /// `Some` once the orchestrator's hello was accepted: the capabilities it accepted.
    accepted: Option<Vec<String>>,
}

fn frame(body: node_body::Body) -> NodeFrame {
    NodeFrame { body: Some(body) }
}

impl<C: Control> NodeSession<C> {
    pub fn new(core: NodeCore, control: C, display_name: impl Into<String>) -> Self {
        Self {
            core,
            control,
            display_name: display_name.into(),
            accepted: None,
        }
    }

    pub fn core(&self) -> &NodeCore {
        &self.core
    }

    pub fn control(&self) -> &C {
        &self.control
    }

    /// The first frame on a (re)connected stream.
    pub fn connect(&mut self, servers: Vec<String>) -> NodeFrame {
        self.accepted = None;
        frame(node_body::Body::Hello(NodeHello {
            version: Some(CURRENT_VERSION),
            node_id: self.core.host().as_str().to_owned(),
            display_name: self.display_name.clone(),
            capabilities: ADVERTISED_CAPABILITIES.map(str::to_owned).to_vec(),
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

    pub fn on_frame(&mut self, incoming: OrchestratorFrame, now: u64) -> SessionOutput {
        if let Err(reject) = incoming.validate() {
            return SessionOutput {
                frames: Vec::new(),
                close: Some(format!("invalid frame from orchestrator: {reject}")),
            };
        }
        match incoming.body {
            Some(orchestrator_body::Body::Hello(h)) => self.on_hello(h),
            Some(orchestrator_body::Body::Heartbeat(h)) => {
                SessionOutput::frames(vec![self.heartbeat(h.seq)])
            }
            Some(orchestrator_body::Body::Resync(_)) => SessionOutput::frames(self.snapshot()),
            Some(orchestrator_body::Body::Request(r)) => {
                SessionOutput::frames(vec![self.on_request(r, now)])
            }
            Some(orchestrator_body::Body::Goodbye(g)) => SessionOutput {
                frames: Vec::new(),
                close: Some(format!("orchestrator closed the stream: {}", g.message)),
            },
            None => SessionOutput::default(),
        }
    }

    fn snapshot(&mut self) -> Vec<NodeFrame> {
        vec![frame(node_body::Body::Snapshot(self.core.snapshot()))]
    }

    fn on_hello(&mut self, hello: flight_proto::OrchestratorHello) -> SessionOutput {
        let theirs = hello
            .version
            .unwrap_or(ProtocolVersion { major: 0, minor: 0 });
        if let Err(reject) = CURRENT_VERSION.negotiate(theirs) {
            return SessionOutput {
                frames: Vec::new(),
                close: Some(reject.to_string()),
            };
        }
        self.accepted = Some(capability::negotiate(
            &hello.accepted_capabilities,
            &ADVERTISED_CAPABILITIES,
        ));
        SessionOutput::frames(self.snapshot())
    }

    fn on_request(&self, request: Request, now: u64) -> NodeFrame {
        let result = match request.command {
            Some(command) => self.execute(&command, now),
            None => Err(ControlError::new(
                ErrorKindCode::InvalidRequest,
                "no command",
            )),
        };
        frame(node_body::Body::Response(Response {
            request_id: request.request_id,
            result: Some(match result {
                Ok(r) => r,
                Err(e) => response_result::Result::Error(e.info()),
            }),
        }))
    }

    fn execute(
        &self,
        command: &Command,
        now: u64,
    ) -> Result<response_result::Result, ControlError> {
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
        let done = || response_result::Result::Done(response_result::Done {});
        match command.kind.as_ref() {
            Some(Kind::GetPreview(c)) => {
                need(capability::PREVIEW)?;
                let pane = self.own_pane(c.pane_ref.as_ref())?;
                let text = self.control.capture(&pane.server, &pane.pane, c.lines)?;
                Ok(response_result::Result::Preview(Preview {
                    text,
                    captured_at: now,
                }))
            }
            Some(Kind::KillPane(c)) => {
                need(capability::KILL)?;
                let pane = self.own_pane(c.pane_ref.as_ref())?;
                self.control.kill_pane(&pane.server, &pane.pane)?;
                Ok(done())
            }
            Some(Kind::CreateSession(c)) => {
                need(capability::CREATE_SESSION)?;
                if c.host != self.core.host().as_str() {
                    return Err(ControlError::new(
                        ErrorKindCode::InvalidRequest,
                        "wrong host",
                    ));
                }
                let server = flight_state::ServerId::new(c.server.as_str());
                self.control
                    .create_session(&server, &c.name, &c.dir, &c.command)?;
                Ok(done())
            }
            Some(Kind::SwitchPane(_)) => Err(ControlError::new(
                ErrorKindCode::Unsupported,
                "switching is not available on a node",
            )),
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

    /// The pane named by a request, only if it is an agent pane this node publishes.
    fn own_pane(&self, msg: Option<&PaneRefMsg>) -> Result<PaneRef, ControlError> {
        let invalid = |m: &str| ControlError::new(ErrorKindCode::InvalidRequest, m.to_owned());
        let pane = PaneRef::try_from(msg.ok_or_else(|| invalid("no pane"))?)
            .map_err(|e| invalid(&e.to_string()))?;
        if pane.host != *self.core.host() {
            return Err(invalid("pane belongs to another host"));
        }
        if !self.core.knows(&pane) {
            return Err(ControlError::new(
                ErrorKindCode::UnknownPane,
                format!("no agent pane {}", pane.pane),
            ));
        }
        Ok(pane)
    }
}
