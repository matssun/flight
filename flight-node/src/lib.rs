// SPDX-License-Identifier: MIT

//! flight-node: the observer that runs beside tmux.
//!
//! ```text
//! tmux adapter -> Round -> NodeCore (classify, fuse, resolve, Tracking, state) -> Snapshot/Delta
//! ```
//!
//! [`NodeCore`] knows nothing about tmux I/O, threads, sockets or clocks: callers hand it
//! [`Round`]s (what was observed, and when). It owns the authoritative current state, never an
//! event history; deltas are derived from changes in that state, so a full [`Snapshot`] is
//! always sufficient. `Tracking` is node-local and never replicated; `PaneState` is.
//!
//! [`Snapshot`]: flight_proto::Snapshot

mod codes;
mod control;
mod entry;
mod incarnation;
mod node_core;
mod node_session;
mod observer;
mod pane_agent;
mod pane_resolve;
mod persistence;
mod round;
mod session_create;
mod terminal;
mod tmux_servers;
mod unavailable;

pub use control::{done_frame, error_frame, Control, ControlError, ControlJob};
pub use incarnation::fresh_incarnation;
pub use node_core::NodeCore;
pub use node_session::{NodeSession, SessionOutput, ADVERTISED_CAPABILITIES};
pub use observer::{ControlLink, ControlSkipObserver, PaneObserver, SequentialObserver};
pub use persistence::{SavedAction, SavedActionRequest, WorkspacePersistence};
pub use round::{PaneObservation, Round, ServerOutcome};
pub use session_create::{NewSurface, Program, SessionEnv, SessionRequest, SurfaceRequest};
pub use terminal::{
    tmux_attach_command, HangUp, OpenedTerminal, Redraw, TerminalProcess, TerminalSpec,
};
pub use tmux_servers::TmuxServers;
pub use unavailable::Unavailable;

pub(crate) use pane_agent::pane_agent;
