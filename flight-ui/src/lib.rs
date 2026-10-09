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

pub use app::{run, run_with_notice, run_with_start, Exit, Start};
pub use collect::{Backend, Collector, CreateFailure};
pub use render::{layout_kind, render, render_to_string, session_at, LayoutKind};
pub use snapshot::{
    HostHealth, HostView, PanePreview, PaneView, SavedHealth, SavedRoot, SavedView, Surface,
    SurfaceKind, UiSnapshot, Workspace, WorkspaceKey,
};
pub use view::{
    unavailable, workspaces, Action, Effect, Field, FilterInput, FormInput, FormOutcome,
    HostChoice, InputMode, NewSessionForm, NewSessionRequest, NewSurfaceRequest, Program,
    PromptButton, PromptInput, PromptOutcome, ShellPrompt, Summary, SurfaceChoice, Tier, ViewModel,
};
