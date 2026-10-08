// SPDX-License-Identifier: MIT

//! Showing a remote pane (ADR-003): the user's terminal is connected, through Flight's own
//! connections, to a tmux client the node runs in a PTY. `relay` is the logic and takes plain
//! channels; `run_terminal` is the glue to the real terminal.

mod end;
mod escape;
mod lease;
mod relay;
mod tty;

pub use end::TerminalEnd;
pub use escape::{EscapeAction, EscapeFilter};
pub use lease::{Lease, LEASE_PERIOD};
pub use relay::relay;
pub use tty::{run_terminal, terminal_request_shape};
