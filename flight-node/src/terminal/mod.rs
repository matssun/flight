// SPDX-License-Identifier: MIT

//! Interactive terminals (ADR-003): a PTY the node owns, running one real tmux client
//! attached to a pane. Synchronous and thread-based; the transport bridges it to a stream.

mod command;
mod process;
mod spec;

pub use command::tmux_attach_command;
pub use process::{OpenedTerminal, TerminalProcess};
pub use spec::TerminalSpec;
