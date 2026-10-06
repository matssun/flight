// SPDX-License-Identifier: MIT

mod agent_detect;
mod collector;
mod resolve_pane;
mod why;

pub use agent_detect::detect_agent;
pub use collector::Collector;
