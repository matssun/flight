// SPDX-License-Identifier: MIT

//! Temporal resolution: previous state + current evidence + time -> new state. Adds what a
//! single fusion cannot know: the glyph debounce and the hook-less Done machine. Ported from
//! Fleet's discovery tier (see THIRD_PARTY.md).

mod done_machine;
mod glyph_debounce;
mod prune;
mod resolve_input;
mod resolved_state;
mod resolver;
mod tracking;
mod why;

pub use prune::prune_tracking;
pub use resolve_input::{ResolveInput, DEFAULT_IDLE_SECS};
pub use resolved_state::{Provenance, ResolvedState};
pub use resolver::resolve;
pub use tracking::Tracking;
pub use why::why;
