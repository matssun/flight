// SPDX-License-Identifier: MIT

//! Switching to a pane (ADR-003): `plan_switch` is a pure decision; the executors do the
//! tmux and ssh work; a failure says whether the reveal or the presentation failed.

mod attach_command;
mod error;
mod handoff;
mod handoff_slot;
mod placement;
mod plan;
mod refusal;
mod remote_ops;
mod switcher;

pub use attach_command::AttachCommand;
pub use error::SwitchError;
pub use handoff::{Handoff, ShownSurface};
pub use handoff_slot::HandoffSlot;
pub use placement::{detect_placement, TmuxEnv, UiPlacement};
pub use plan::{plan_switch, SwitchPlan, SwitchTarget, UiContext};
pub use refusal::Refusal;
pub use remote_ops::RemoteOps;
pub use switcher::{Presented, Switcher};
