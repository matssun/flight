// SPDX-License-Identifier: MIT

//! flight-tmux — thin, testable tmux IPC. See docs/adr/ADR-001-architecture.md.

mod client;
mod control;
mod endpoint;
mod error;
mod pane_info;
mod parse;
mod runner;

pub use client::Tmux;
pub use control::{ControlConnection, ControlReply};
pub use endpoint::TmuxEndpoint;
pub use error::TmuxError;
pub use pane_info::PaneInfo;
pub use parse::{parse_panes_checked, parse_panes_output, PANE_FORMAT};
pub use runner::{tmux_args, SystemRunner, TmuxOutput, TmuxRunner};
