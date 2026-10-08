// SPDX-License-Identifier: MIT

use crate::{ExitReasonCode, Reject, Validate};

/// The most bytes one `TerminalData` frame may carry. Checked where a frame enters, before it
/// reaches any queue.
pub const MAX_TERMINAL_DATA: usize = 16 * 1024;
/// The largest terminal dimension (columns or rows) a peer may ask for.
pub const MAX_TERMINAL_DIM: u32 = 1000;
/// A terminal id is 128 random bits minted by the orchestrator.
pub const TERMINAL_ID_LEN: usize = 16;
/// The longest `TERM` value accepted.
pub const MAX_TERM_LEN: usize = 32;

/// `TERM` is bounded and made of `[A-Za-z0-9._-]` only: it becomes an environment value on a
/// node and nothing else.
pub fn valid_term(term: &str) -> bool {
    !term.is_empty()
        && term.len() <= MAX_TERM_LEN
        && term
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn valid_dims(cols: u32, rows: u32) -> bool {
    (1..=MAX_TERMINAL_DIM).contains(&cols) && (1..=MAX_TERMINAL_DIM).contains(&rows)
}

/// The first frame of a terminal stream, from whichever side dialled.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalAttach {
    #[prost(bytes = "vec", tag = "1")]
    pub terminal_id: Vec<u8>,
}

/// A UI's terminal presentation is alive (ADR-004). Sent on the UI's control stream so that
/// terminal backpressure cannot delay it.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalLease {
    #[prost(bytes = "vec", tag = "1")]
    pub terminal_id: Vec<u8>,
}

/// Opaque terminal bytes: output when a node sent them, keystrokes when a UI did. Nobody
/// between the PTY and the user's terminal interprets them.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalData {
    #[prost(bytes = "vec", tag = "1")]
    pub payload: Vec<u8>,
}

/// UI to node: the user's terminal changed size.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalResize {
    #[prost(uint32, tag = "1")]
    pub cols: u32,
    #[prost(uint32, tag = "2")]
    pub rows: u32,
}

/// Node to UI, the last frame: the terminal has ended and why.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalExit {
    #[prost(enumeration = "ExitReasonCode", tag = "1")]
    pub reason: i32,
    /// The tmux client's exit status when it exited on its own; 0 otherwise.
    #[prost(int32, tag = "2")]
    pub status: i32,
}

/// UI to node, the last frame: the user is done.
#[derive(Clone, Copy, PartialEq, Eq, prost::Message)]
pub struct TerminalClose {}

/// Both directions of a terminal stream (`TerminalNode`, `TerminalUi`).
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalFrame {
    #[prost(oneof = "terminal_body::Body", tags = "1, 2, 3, 4, 5")]
    pub body: Option<terminal_body::Body>,
}

pub mod terminal_body {
    use crate::{TerminalAttach, TerminalClose, TerminalData, TerminalExit, TerminalResize};

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "1")]
        Attach(TerminalAttach),
        #[prost(message, tag = "2")]
        Data(TerminalData),
        #[prost(message, tag = "3")]
        Resize(TerminalResize),
        #[prost(message, tag = "4")]
        Exit(TerminalExit),
        #[prost(message, tag = "5")]
        Close(TerminalClose),
    }
}

/// Which end of a terminal stream sent a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Ui,
    Node,
}

impl TerminalFrame {
    pub fn attach(terminal_id: Vec<u8>) -> Self {
        Self::of(terminal_body::Body::Attach(TerminalAttach { terminal_id }))
    }

    pub fn data(payload: Vec<u8>) -> Self {
        Self::of(terminal_body::Body::Data(TerminalData { payload }))
    }

    fn of(body: terminal_body::Body) -> Self {
        Self { body: Some(body) }
    }

    /// Whether this frame ends the stream.
    pub fn is_last(&self) -> bool {
        matches!(
            self.body,
            Some(terminal_body::Body::Exit(_) | terminal_body::Body::Close(_))
        )
    }

    /// Validate a frame and check that `origin` is allowed to send it: a node cannot send
    /// `resize` or `close`, a UI cannot send `exit`.
    pub fn validate_from(&self, origin: Origin) -> Result<(), Reject> {
        self.validate()?;
        use terminal_body::Body::*;
        let allowed = match self.body.as_ref() {
            Some(Attach(_) | Data(_)) => true,
            Some(Resize(_) | Close(_)) => origin == Origin::Ui,
            Some(Exit(_)) => origin == Origin::Node,
            None => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(Reject::Mismatch("terminal_frame.direction"))
        }
    }
}

impl Validate for TerminalFrame {
    fn validate(&self) -> Result<(), Reject> {
        use terminal_body::Body::*;
        match self
            .body
            .as_ref()
            .ok_or(Reject::Missing("terminal_frame.body"))?
        {
            Attach(a) => {
                if a.terminal_id.len() == TERMINAL_ID_LEN {
                    Ok(())
                } else {
                    Err(Reject::OutOfRange("terminal_attach.terminal_id"))
                }
            }
            Data(d) => {
                if d.payload.len() <= MAX_TERMINAL_DATA {
                    Ok(())
                } else {
                    Err(Reject::TooLarge {
                        len: d.payload.len(),
                        max: MAX_TERMINAL_DATA,
                    })
                }
            }
            Resize(r) => {
                if valid_dims(r.cols, r.rows) {
                    Ok(())
                } else {
                    Err(Reject::OutOfRange("terminal_resize"))
                }
            }
            Exit(e) => ExitReasonCode::decode(e.reason, "terminal_exit.reason").map(|_| ()),
            Close(_) => Ok(()),
        }
    }
}

/// The answer to a successful `OpenTerminal`: the id both ends now attach with.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct TerminalOpened {
    #[prost(bytes = "vec", tag = "1")]
    pub terminal_id: Vec<u8>,
}
