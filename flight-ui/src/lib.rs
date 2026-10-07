// SPDX-License-Identifier: MIT

//! flight-ui — the dashboard.
//!
//! `UiSnapshot` (data, built by the [`Collector`]) -> [`ViewModel`] (pure presentation
//! state) -> ratatui rendering (a pure function of the view model) -> terminal loop.
//! The loop owns I/O, the view model owns presentation state, rendering owns nothing.

mod app;
mod collect;
mod render;
mod snapshot;
mod view;

pub use app::{run, Exit};
pub use collect::{Backend, Collector};
pub use render::{render, render_to_string};
pub use snapshot::{HostHealth, HostView, PanePreview, PaneView, UiSnapshot};
pub use view::{attention_panes, section_panes, tree_panes, Action, Effect, Section, ViewModel};
