// SPDX-License-Identifier: MIT

//! flight-classify — pure classification of pane observations into [`AgentState`].
//!
//! Performs no observation: callers (tmux, SSH, a future daemon) build an [`Observation`]
//! and this crate deterministically maps it to a [`Classification`] carrying the id of the
//! rule that fired. Rule manifests are ported from Fleet's detection manifests
//! (src/state/detection.ts, MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md) with unchanged
//! meaning. Ordering is explicit data: each rule has a priority, lowest first.
//!
//! Not in scope here: fusing hook/event evidence with scrape results (Fleet's engine.ts).

mod agent_kind;
mod ansi;
mod builtin;
mod classification;
mod error;
mod manifest;
mod observation;
mod refine;
mod rule;
mod rule_id;
mod screen;
mod title;

pub use agent_kind::AgentKind;
pub use classification::Classification;
pub use error::ClassifyError;
pub use manifest::Manifest;
pub use observation::Observation;
pub use refine::refine_title_with_screen;
pub use rule::Rule;
pub use rule_id::{RuleId, PROMPT_MARKER_RULE_ID};
pub use screen::classify_screen;
pub use title::classify_title;
