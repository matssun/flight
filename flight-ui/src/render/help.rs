// SPDX-License-Identifier: MIT

use super::style::{bold, dim, key, state_look, state_meaning};
use flight_state::AgentState;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// The help overlay: moving around, what the symbols mean, and what can be done.
pub fn help_lines() -> Vec<Line<'static>> {
    let mut out = vec![Line::raw("")];
    out.push(heading("Moving around"));
    for (k, text) in [
        ("↑ ↓  j k", "move between workspaces"),
        ("Enter  a", "open the workspace's agent"),
        ("s", "open its shell (offers to create one)"),
        ("Ctrl-Space a / s", "switch to the agent / shell"),
        ("Ctrl-Space q", "back to the dashboard; both keep running"),
        ("/", "search by name, host or agent"),
        ("Esc", "clear the search, or quit"),
    ] {
        out.push(entry(k, text));
    }
    out.push(Line::raw(""));
    out.push(heading("What the symbols mean (most urgent first)"));
    for s in [
        AgentState::Permit,
        AgentState::Question,
        AgentState::Done,
        AgentState::Busy,
        AgentState::Idle,
        AgentState::Shell,
        AgentState::Down,
    ] {
        let (icon, label, colour) = state_look(s);
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{icon} {label:<9}"), Style::default().fg(colour)),
            Span::styled(state_meaning(s).to_owned(), dim()),
        ]));
    }
    out.push(Line::raw(""));
    out.push(heading("Actions"));
    for (k, text) in [
        ("n", "new workspace (host, folder, Claude)"),
        ("r", "refresh"),
        ("q", "quit"),
    ] {
        out.push(entry(k, text));
    }
    out.push(Line::raw(""));
    out.push(Line::styled("  Press any key to close", dim()));
    out
}

fn heading(text: &'static str) -> Line<'static> {
    Line::styled(format!(" {text}"), bold())
}

fn entry(k: &'static str, text: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{k:<18}"), key()),
        Span::styled(text.to_owned(), dim()),
    ])
}
