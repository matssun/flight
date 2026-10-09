// SPDX-License-Identifier: MIT

//! flight-tmux — thin, testable tmux IPC. See docs/adr/ADR-001-architecture.md.

mod client;
mod client_ops;
mod control;
mod create;
mod create_error;
mod create_window;
mod endpoint;
mod error;
mod new_session;
mod pane_info;
mod parse;
mod runner;
mod surface_mark;
mod view;

pub use client::Tmux;
pub use client_ops::ClientInfo;
pub use control::{ControlConnection, ControlReply};
pub use create::FLIGHT_SESSION_OPTION;
pub use create_error::CreateError;
pub use endpoint::TmuxEndpoint;
pub use error::TmuxError;
pub use new_session::{Launch, NewSession};
pub use pane_info::PaneInfo;
pub use parse::{parse_panes_checked, parse_panes_output, PANE_FORMAT};
pub use runner::{tmux_args, SystemRunner, TmuxOutput, TmuxRunner};
pub use surface_mark::{
    ConfigMark, SurfaceMark, SurfaceTag, CONFIG_OPTION, CONFIG_SURFACE_OPTION, SURFACE_ID_OPTION,
    SURFACE_OPTION, WORKSPACE_OPTION,
};
pub use view::{is_view_session, view_session_name, VIEW_SESSION_PREFIX};
