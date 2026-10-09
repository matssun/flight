// SPDX-License-Identifier: MIT

//! The companion-shell prompt as plain lines. A pure function of the prompt.

use super::style::{bold, dim, key};
use super::text::fit;
use crate::view::{PromptButton, ShellPrompt};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Room for the directory inside the modal.
const ROOM: usize = 60;

pub fn prompt_lines(prompt: &ShellPrompt) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{} has no shell yet.", prompt.name()), bold()),
        ]),
        Line::raw(""),
        Line::styled(
            format!(
                "  Create a shell in {} on {}?",
                fit(prompt.root(), ROOM.saturating_sub(24)),
                prompt.host_label()
            ),
            Style::default(),
        ),
        Line::styled("  It starts where the agent does and keeps running.", dim()),
        Line::raw(""),
    ];
    lines.push(match (prompt.submitting(), prompt.error()) {
        (true, _) => Line::styled("  Creating the shell…", dim()),
        (false, Some(e)) => Line::styled(format!("  {e}"), Style::default().fg(Color::Red)),
        (false, None) => Line::raw(""),
    });
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(12)),
        button(prompt, PromptButton::Create, "Create"),
        Span::raw("  "),
        button(prompt, PromptButton::Cancel, "Cancel"),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  y", key()),
        Span::styled(" create   ", dim()),
        Span::styled("Enter", key()),
        Span::styled(" confirm   ", dim()),
        Span::styled("Esc", key()),
        Span::styled(" cancel", dim()),
    ]));
    lines
}

fn button(prompt: &ShellPrompt, which: PromptButton, label: &str) -> Span<'static> {
    if prompt.focus() == which {
        Span::styled(
            format!(" {label} "),
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {label} "), Style::default().fg(Color::Cyan))
    }
}
