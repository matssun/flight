// SPDX-License-Identifier: MIT

//! flight-client: the dashboard backend over an orchestrator.
//!
//! `UiClient` (gRPC over mutual TLS) -> `FleetImage` (snapshot + ordered deltas) ->
//! `UiSnapshot` -> `flight-ui`. The dashboard is configured with an orchestrator endpoint and a
//! pinned identity; it cannot tell a local orchestrator from a LAN one or a hosted one.

mod orchestrated;
mod snapshot_view;
mod switch;
mod terminal;

pub use orchestrated::{ClientConfig, OrchestratedBackend};
pub use snapshot_view::ui_snapshot;
pub use switch::{
    detect_placement, plan_switch, AttachCommand, Handoff, HandoffSlot, Presented, Refusal,
    RemoteOps, SwitchError, SwitchPlan, SwitchTarget, Switcher, TmuxEnv, UiContext, UiPlacement,
};
pub use terminal::{
    relay, run_terminal, terminal_request_shape, EscapeAction, EscapeFilter, TerminalEnd,
};
