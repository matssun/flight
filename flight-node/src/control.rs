// SPDX-License-Identifier: MIT

use flight_proto::{ErrorInfo, ErrorKindCode};
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

/// What a node can do on request, against its own tmux servers. The session checks identity,
/// capability and that the pane is one the node publishes before calling these.
pub trait Control {
    /// The last `lines` lines of the pane's visible screen (plain text).
    fn capture(&self, server: &ServerId, pane: &PaneId, lines: u32)
        -> Result<String, ControlError>;
    fn kill_pane(&self, server: &ServerId, pane: &PaneId) -> Result<(), ControlError>;
    /// A detached session; `command` empty means the shell.
    fn create_session(
        &self,
        server: &ServerId,
        name: &str,
        dir: &str,
        command: &str,
    ) -> Result<(), ControlError>;
}
