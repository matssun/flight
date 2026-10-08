// SPDX-License-Identifier: MIT

//! Switching to a pane (ADR-003): `plan_switch` is a pure decision; the executors do the
//! tmux and ssh work; a failure says whether the reveal or the presentation failed.

mod error;
mod handoff;
mod handoff_slot;
mod placement;
mod plan;
mod refusal;
mod ssh_destinations;
mod switcher;

pub use error::SwitchError;
pub use handoff::Handoff;
pub use handoff_slot::HandoffSlot;
pub use placement::{detect_placement, TmuxEnv, UiPlacement};
pub use plan::{plan_switch, SwitchPlan, SwitchTarget, UiContext};
pub use refusal::Refusal;
pub use ssh_destinations::{SshDestinations, SshDestinationsError};
pub use switcher::{Presented, Switcher};
