// SPDX-License-Identifier: MIT

//! The saved-workspace questions as plain lines. A pure function of the prompt.

use super::style::{bold, dim, key};
use super::text::fit;
use crate::view::{SavedPrompt, SavedPromptButton, SavedPromptKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const ROOM: usize = 62;

/// The modal's title.
pub fn title(prompt: &SavedPrompt) -> &'static str {
    match prompt.kind() {
        SavedPromptKind::Remove => "Forget saved workspace",
        SavedPromptKind::AcceptRoot => "Accept this directory",
        SavedPromptKind::Trust => "Trust imported workspace",
        SavedPromptKind::ChangeRoot(_) => "Change directory",
    }
}

pub fn saved_prompt_lines(prompt: &SavedPrompt) -> Vec<Line<'static>> {
    let s = prompt.saved();
    let plain = Style::default();
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{} on {}", s.name, s.host_label), bold()),
        ]),
        Line::styled(format!("  {}", fit(&s.root, ROOM)), dim()),
        Line::raw(""),
    ];
    match prompt.kind() {
        SavedPromptKind::Remove => {
            lines.push(Line::styled("  Forget this saved workspace?", plain));
            lines.push(Line::styled(
                "  Only the saved entry goes. Running processes, files and",
                dim(),
            ));
            lines.push(Line::styled("  repositories are not touched.", dim()));
        }
        SavedPromptKind::AcceptRoot => {
            lines.push(Line::styled(
                "  A different directory is at this path now.",
                plain,
            ));
            lines.push(Line::styled(
                "  Is it the one this workspace should use from now on?",
                plain,
            ));
            lines.push(Line::styled(
                "  Nothing is created or changed in it.",
                dim(),
            ));
        }
        SavedPromptKind::Trust => {
            lines.push(Line::styled("  This workspace came from an import.", plain));
            lines.push(Line::styled(
                "  Trust it, so it can be started on this host?",
                plain,
            ));
            lines.push(Line::styled(
                "  Starting it still needs your Enter on it.",
                dim(),
            ));
        }
        SavedPromptKind::ChangeRoot(text) => {
            lines.push(Line::styled("  Point it at this directory:", plain));
            lines.push(Line::from(vec![
                Span::styled("  ❯ ", Style::default().fg(Color::Cyan)),
                Span::raw(fit(text, ROOM.saturating_sub(6))),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ]));
            lines.push(Line::styled(
                "  It is checked on the host; nothing is created there.",
                dim(),
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.push(match (prompt.submitting(), prompt.error()) {
        (true, _) => Line::styled("  Working…", dim()),
        (false, Some(e)) => Line::styled(
            format!("  {}", fit(e, ROOM)),
            Style::default().fg(Color::Red),
        ),
        (false, None) => Line::raw(""),
    });
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(12)),
        button(prompt, SavedPromptButton::Confirm, confirm_label(prompt)),
        Span::raw("  "),
        button(prompt, SavedPromptButton::Cancel, "Cancel"),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  Enter", key()),
        Span::styled(" press the button   ", dim()),
        Span::styled("Tab", key()),
        Span::styled(" other button   ", dim()),
        Span::styled("Esc", key()),
        Span::styled(" cancel", dim()),
    ]));
    lines
}

fn confirm_label(prompt: &SavedPrompt) -> &'static str {
    match prompt.kind() {
        SavedPromptKind::Remove => "Forget",
        SavedPromptKind::AcceptRoot => "Accept",
        SavedPromptKind::Trust => "Trust",
        SavedPromptKind::ChangeRoot(_) => "Change",
    }
}

fn button(prompt: &SavedPrompt, which: SavedPromptButton, label: &str) -> Span<'static> {
    if prompt.focus() == which {
        Span::styled(
            format!(" {label} "),
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {label} "), Style::default().fg(Color::Cyan))
    }
}
