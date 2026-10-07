// SPDX-License-Identifier: MIT

use flight_tmux::{ControlConnection, ControlReply, TmuxError};

/// A persistent control channel to one tmux server: commands go in as a pipelined batch and
/// come back as one reply each, in order. Any error means the channel can no longer be
/// trusted and must be replaced.
pub trait ControlLink: Send {
    fn run(&mut self, commands: &[String]) -> Result<Vec<ControlReply>, TmuxError>;

    /// The process id of this channel's own tmux client (`#{client_pid}`), so its presence
    /// can be discounted from "a client is attached".
    fn client_pid(&self) -> u32;
}

impl ControlLink for ControlConnection {
    fn run(&mut self, commands: &[String]) -> Result<Vec<ControlReply>, TmuxError> {
        ControlConnection::run(self, commands)
    }

    fn client_pid(&self) -> u32 {
        ControlConnection::client_pid(self)
    }
}
