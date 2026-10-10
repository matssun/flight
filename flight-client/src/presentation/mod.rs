// SPDX-License-Identifier: MIT

//! Several surfaces on one terminal (ADR-011).
//!
//! A [`Layout`](flight_present::Layout) says which surfaces are on screen and where. A
//! [`PresentationSession`] attaches exactly the surfaces that have a tile, keeps a screen for
//! each, paints them together, sends the keyboard to the focused one and each attachment the
//! size of its own tile. Surfaces, links and attachments keep the lifetimes ADR-009 gives
//! them; the layout decides none of them.

mod arrangement;
mod command;
mod config;
mod frame;
mod input_log;
mod keys;
mod mouse_event;
mod mouse_filter;
mod outcome;
mod pointer;
mod presentation_session;
mod rebuild_budget;
mod surface_names;
mod target;
mod tile_link;
pub(crate) mod workspace_surfaces;

#[cfg(test)]
mod presentation_tests;

pub use arrangement::{remember, side_by_side, starting_layout};
pub use command::{Command, Shown};
pub use config::PresentationConfig;
pub use keys::{Key, KeyFilter};
pub use outcome::PresentationOutcome;
pub use presentation_session::PresentationSession;
pub use surface_names::surface_id;
pub use target::Target;
pub use workspace_surfaces::WorkspaceSurfaces;
