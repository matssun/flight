// SPDX-License-Identifier: MIT

//! flight-control — route operations to hosts by identity, with typed failures.
//!
//! `PaneRef -> HostId -> transport -> TmuxEndpoint -> flight-tmux`. Transports: local, and
//! SSH via an OpenSSH host alias. No scheduling, no persistence. See
//! docs/adr/ADR-001-architecture.md.

mod classify;
mod host_error;
mod host_status;
mod ops;
mod panes_outcome;
mod probe;
mod registry;
mod shell_quote;
mod ssh_runner;
mod transport;

pub use classify::classify;
pub use host_error::HostError;
pub use host_status::HostStatus;
pub use panes_outcome::{HostPane, PanesOutcome};
pub use registry::{BoxedRunner, HostRegistry};
pub use ssh_runner::SshRunner;
pub use transport::Transport;
