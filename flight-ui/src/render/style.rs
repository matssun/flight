// SPDX-License-Identifier: MIT

//! How states and health look. Presentation only: the meaning lives in `flight-state`.

use crate::snapshot::HostHealth;
use flight_state::AgentState;
use ratatui::style::Color;

/// (icon, label, colour). Icons and labels follow Fleet.
pub fn state_look(state: AgentState) -> (&'static str, &'static str, Color) {
    match state {
        AgentState::Permit => ("⚠", "waiting", Color::Red),
        AgentState::Question => ("?", "asking", Color::Red),
        AgentState::Done => ("●", "ready", Color::Green),
        AgentState::Busy => ("⠋", "working", Color::Yellow),
        AgentState::Idle => ("○", "idle", Color::DarkGray),
        AgentState::Shell => ("■", "shell", Color::DarkGray),
        AgentState::Down => ("○", "down", Color::DarkGray),
    }
}

/// (icon, text, colour) for a host row.
pub fn health_look(h: &HostHealth) -> (&'static str, String, Color) {
    match h {
        HostHealth::Online => ("●", "online".to_owned(), Color::Green),
        HostHealth::Stale => ("!", "stale (last known)".to_owned(), Color::Yellow),
        HostHealth::Disconnected => ("!", "disconnected (last known)".to_owned(), Color::Red),
        HostHealth::NoServer => ("○", "no sessions yet".to_owned(), Color::DarkGray),
        HostHealth::Unreachable(_) => ("!", "unreachable".to_owned(), Color::Red),
        HostHealth::AuthFailed(_) => ("!", "authentication failed".to_owned(), Color::Red),
        HostHealth::NoTmux => ("!", "session backend missing".to_owned(), Color::Red),
        HostHealth::Failed(_) => ("!", "error".to_owned(), Color::Red),
    }
}
