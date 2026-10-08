// SPDX-License-Identifier: MIT

use crate::{ErrorKindCode, Reject, Validate};

/// Ask the node for a fresh [`crate::Snapshot`]. Sent on any gap, incarnation mismatch or
/// reconnect; the orchestrator never tries to repair a stream.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct ResyncRequest {
    #[prost(string, tag = "1")]
    pub reason: String,
}

/// The orchestrator is closing the stream and says why.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Goodbye {
    #[prost(enumeration = "ErrorKindCode", tag = "1")]
    pub reason: i32,
    #[prost(string, tag = "2")]
    pub message: String,
}

/// Everything the orchestrator sends a node on the same stream.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct OrchestratorFrame {
    #[prost(oneof = "orchestrator_body::Body", tags = "1, 2, 3, 4, 5")]
    pub body: Option<orchestrator_body::Body>,
}

pub mod orchestrator_body {
    use crate::{Goodbye, Heartbeat, OrchestratorHello, Request, ResyncRequest};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Hello(OrchestratorHello),
        #[prost(message, tag = "2")]
        Heartbeat(Heartbeat),
        #[prost(message, tag = "3")]
        Resync(ResyncRequest),
        #[prost(message, tag = "4")]
        Request(Request),
        #[prost(message, tag = "5")]
        Goodbye(Goodbye),
    }
}

impl Validate for OrchestratorFrame {
    fn validate(&self) -> Result<(), Reject> {
        use orchestrator_body::Body::*;
        match self
            .body
            .as_ref()
            .ok_or(Reject::Missing("orchestrator_frame.body"))?
        {
            Hello(m) => m.validate(),
            Heartbeat(m) => m.validate(),
            Resync(_) => Ok(()),
            Request(m) => {
                m.validate()?;
                if let Some(crate::command_kind::Kind::OpenTerminal(open)) =
                    m.command.as_ref().and_then(|c| c.kind.as_ref())
                {
                    if open.terminal_id.len() != crate::TERMINAL_ID_LEN {
                        return Err(Reject::Missing("open_terminal.terminal_id"));
                    }
                }
                Ok(())
            }
            Goodbye(g) => ErrorKindCode::decode(g.reason, "goodbye.reason").map(|_| ()),
        }
    }
}
