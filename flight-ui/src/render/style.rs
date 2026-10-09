// SPDX-License-Identifier: MIT

//! How states and health look. Presentation only: the meaning lives in `flight-state`.

use crate::snapshot::{HostHealth, Surface, SurfaceKind};
use crate::view::Tier;
use flight_state::AgentState;
use ratatui::style::{Color, Modifier, Style};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

/// The icon as shown now: only the working spinner moves.
pub fn state_icon(state: AgentState, frame: u32) -> &'static str {
    if state == AgentState::Busy {
        let i = usize::try_from(frame).unwrap_or(0) % SPINNER.len();
        return SPINNER.get(i).copied().unwrap_or("⠋");
    }
    state_look(state).0
}

/// What a state means, in words a first-time user can act on.
pub fn state_meaning(state: AgentState) -> &'static str {
    match state {
        AgentState::Permit => "needs your approval",
        AgentState::Question => "has a question for you",
        AgentState::Done => "finished, your move",
        AgentState::Busy => "thinking or running tools",
        AgentState::Idle => "up, nothing happening",
        AgentState::Shell => "a plain shell, no agent",
        AgentState::Down => "no live process",
    }
}

pub fn tier_colour(tier: Tier) -> Color {
    match tier {
        Tier::NeedsYou => Color::Red,
        Tier::Working => Color::Yellow,
        Tier::Quiet => Color::DarkGray,
    }
}

pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// The key in a hint: bright, so controls stand out from their labels.
pub fn key() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_row() -> Style {
    Style::default().bg(Color::Indexed(238))
}

/// (icon, text, colour) for a host row.
pub fn health_look(h: &HostHealth) -> (&'static str, String, Color) {
    match h {
        HostHealth::Online => ("●", "online".to_owned(), Color::Green),
        HostHealth::Stale => ("!", "no news lately (last known)".to_owned(), Color::Yellow),
        HostHealth::Disconnected => ("!", "disconnected (last known)".to_owned(), Color::Red),
        HostHealth::NoServer => ("○", "no workspaces yet".to_owned(), Color::DarkGray),
        HostHealth::Unreachable(_) => ("!", "unreachable".to_owned(), Color::Red),
        HostHealth::AuthFailed(_) => ("!", "authentication failed".to_owned(), Color::Red),
        HostHealth::NoTmux => ("!", "cannot run workspaces".to_owned(), Color::Red),
        HostHealth::Failed(_) => ("!", "error".to_owned(), Color::Red),
    }
}

/// What to say of a surface and in which colour. A shell with nothing running in it (the
/// node calls that a shell, or idle once it has been quiet a while) is "ready"; an agent, or
/// anything else running in a shell surface, is described by its state.
pub fn surface_status(s: &Surface) -> (&'static str, Color) {
    let state = s.pane.state;
    if s.kind == SurfaceKind::Shell && matches!(state, AgentState::Shell | AgentState::Idle) {
        return ("ready", Color::Green);
    }
    let (_, label, colour) = state_look(state);
    (label, colour)
}
