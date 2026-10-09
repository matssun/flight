// SPDX-License-Identifier: MIT

//! Showing a remote pane (ADR-003, ADR-009): the user's terminal is connected, through Flight's
//! own connections, to a tmux client the node runs in a PTY. The session logic lives in
//! `crate::session` and takes plain channels; `run_session` is the glue to the real terminal.

mod end;
mod escape;
mod local_terminal;
mod tty;

pub use end::TerminalEnd;
pub use escape::EscapeFilter;
pub use local_terminal::LocalTerminal;
pub use tty::{run_session, terminal_request_shape, SessionRequest};

/// How often a presentation renews its terminal's lease. The orchestrator lets a lease run
/// for three of these.
pub const LEASE_PERIOD: std::time::Duration = std::time::Duration::from_secs(5);
