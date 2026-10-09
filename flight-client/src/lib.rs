// SPDX-License-Identifier: MIT

//! flight-client: the dashboard backend over an orchestrator.
//!
//! `UiClient` (gRPC over mutual TLS) -> `FleetImage` (snapshot + ordered deltas) ->
//! `UiSnapshot` -> `flight-ui`. The dashboard is configured with an orchestrator endpoint and a
//! pinned identity; it cannot tell a local orchestrator from a LAN one or a hosted one.

mod link_host;
mod orchestrated;
mod session;
mod snapshot_view;
mod switch;
mod terminal;

pub use link_host::LinkHost;
pub use orchestrated::{ClientConfig, OrchestratedBackend};
pub use session::{
    Attachment, Binding, FromRemote, InputEvent, InputQueue, OpenFailure, OpenRequest,
    SessionConfig, SessionOutcome, SessionStart, SurfaceHost, SurfaceSession, ToRemote,
};
pub use snapshot_view::ui_snapshot;
pub use switch::{
    detect_placement, plan_switch, AttachCommand, Handoff, HandoffSlot, Presented, Refusal,
    RemoteOps, ShownSurface, SwitchError, SwitchPlan, SwitchTarget, Switcher, TmuxEnv, UiContext,
    UiPlacement,
};
pub use terminal::{
    run_session, terminal_request_shape, EscapeFilter, LocalTerminal, SessionRequest, TerminalEnd,
    LEASE_PERIOD,
};
