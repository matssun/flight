// SPDX-License-Identifier: MIT

//! The surface session: what the user's terminal is connected to while they work in a
//! workspace (ADR-009).
//!
//! Three lifetimes are kept apart. A *surface* lives in its backend and is never touched here.
//! A *link* (the orchestrator connection the process already holds) outlives any one surface
//! shown. An *attachment* is one terminal stream to one surface, opened when the surface is
//! shown and ended when it is not. The session owns the user's input and decides, in order,
//! which attachment each key belongs to.

mod attachment;
mod host;
mod input_event;
mod input_queue;
mod outcome;
mod pump;
#[cfg(test)]
mod session_tests;
mod surface_session;

pub use attachment::{Attachment, Binding, FromRemote, ToRemote};
pub use host::{OpenFailure, OpenRequest, SurfaceHost};
pub use input_event::InputEvent;
pub use input_queue::InputQueue;
pub use outcome::SessionOutcome;
pub use surface_session::{SessionConfig, SessionStart, SurfaceSession};
