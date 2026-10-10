// SPDX-License-Identifier: MIT

//! flight-client: the dashboard backend over an orchestrator.
//!
//! `UiClient` (gRPC over mutual TLS) -> `FleetImage` (snapshot + ordered deltas) ->
//! `UiSnapshot` -> `flight-ui`. The dashboard is configured with an orchestrator endpoint and a
//! pinned identity; it cannot tell a local orchestrator from a LAN one or a hosted one.

mod layout_store;
mod link_host;
mod orchestrated;
mod presentation;
mod screens;
mod session;
mod snapshot_view;
mod switch;
mod terminal;
mod visit;

pub use layout_store::LayoutStore;
pub use link_host::LinkHost;
pub use orchestrated::{ClientConfig, OrchestratedBackend};
pub use presentation::{
    remember, side_by_side, starting_layout, Command, Key, KeyFilter, PresentationConfig,
    PresentationOutcome, PresentationSession, Shown,
};
pub use screens::{
    paint, CellView, Colour, EngineFailure, Geometry, Modes, MouseEncoding, MouseMode, Painted,
    ScreenModel, Theme, TooSmall,
};
pub use session::{
    Attachment, Binding, FromRemote, InputEvent, InputQueue, OpenFailure, OpenRequest,
    SessionConfig, SessionOutcome, SessionStart, SurfaceHost, SurfaceSession, ToRemote,
};
pub use snapshot_view::ui_snapshot;
pub use switch::{
    detect_placement, plan_switch, AttachCommand, Handoff, HandoffSlot, Presented, Refusal,
    RemoteError, RemoteOps, ShownSurface, SwitchError, SwitchPlan, SwitchTarget, Switcher, TmuxEnv,
    UiContext, UiPlacement,
};
pub use terminal::{
    run_presentation, run_session, terminal_request_shape, EscapeFilter, LocalTerminal,
    SessionRequest, TerminalEnd, TerminalHold, LEASE_PERIOD,
};
pub use visit::{visit, Returning, Terminals};
