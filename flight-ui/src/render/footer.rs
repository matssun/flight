// SPDX-License-Identifier: MIT

use super::style::{dim, key, state_look};
use super::text::cells;
use crate::view::{InputMode, ViewModel};
use flight_state::AgentState;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// What the symbols mean: the first footer line.
pub fn legend_line(width: usize) -> Line<'static> {
    let items: Vec<Vec<Span<'static>>> = [
        AgentState::Permit,
        AgentState::Question,
        AgentState::Done,
        AgentState::Busy,
        AgentState::Idle,
        AgentState::Shell,
    ]
    .into_iter()
    .map(|s| {
        let (icon, label, colour) = state_look(s);
        vec![
            Span::styled(icon.to_owned(), Style::default().fg(colour)),
            Span::styled(format!(" {label}"), dim()),
        ]
    })
    .collect();
    fitted(items, "  ", width, vec![Span::raw(" ")])
}

/// The controls: the second footer line, or while searching the search itself.
pub fn hints_line(vm: &ViewModel, width: usize) -> Line<'static> {
    if vm.searching() || !vm.filter().is_empty() {
        return search_line(vm);
    }
    let hint = |k: &str, label: &str| {
        vec![
            Span::styled(k.to_owned(), key()),
            Span::styled(format!(" {label}"), dim()),
        ]
    };
    if let Some(saved) = vm.selected_saved() {
        // What can be done to a saved workspace, most useful first.
        let mut items = vec![
            hint("Enter", "Start"),
            hint("r", "Retry"),
            hint("c", "Change dir"),
        ];
        if saved.root_state == crate::snapshot::SavedRoot::Changed {
            items.push(hint("v", "Accept dir"));
        }
        if saved.imported {
            items.push(hint("t", "Trust"));
        }
        items.extend([hint("x", "Forget"), hint("?", "Help"), hint("↑↓", "Move")]);
        return fitted(items, "   ", width, vec![Span::raw(" ")]);
    }
    // Most important first: what does not fit is dropped from the right.
    let items = vec![
        hint("n", "New"),
        hint("Enter", "Open"),
        hint("a", "Agent"),
        hint("s", "Shell"),
        hint("/", "Search"),
        hint("?", "Help"),
        hint("q", "Quit"),
        hint("↑↓", "Move"),
    ];
    fitted(items, "   ", width, vec![Span::raw(" ")])
}

fn search_line(vm: &ViewModel) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("/", key()),
        Span::styled(vm.filter().to_owned(), Style::default().fg(Color::Cyan)),
    ];
    if vm.input_mode() == InputMode::Search {
        spans.push(Span::styled("█", Style::default().fg(Color::Cyan)));
        spans.push(Span::styled("   Enter", key()));
        spans.push(Span::styled(" keep", dim()));
    }
    spans.push(Span::styled("   Esc", key()));
    spans.push(Span::styled(" clear", dim()));
    Line::from(spans)
}

/// A one-off message (for example a created session) in place of the legend.
pub fn message_line(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(text.to_owned(), Style::default().fg(Color::Cyan)),
    ])
}

/// `items` joined by `sep`, as many as fit in `width`.
fn fitted(
    items: Vec<Vec<Span<'static>>>,
    sep: &str,
    width: usize,
    lead: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = lead;
    let mut used: usize = spans.iter().map(Span::width).sum();
    let mut first = true;
    for item in items {
        let w: usize = item.iter().map(Span::width).sum();
        let need = w.saturating_add(if first { 0 } else { cells(sep) });
        if used.saturating_add(need) > width {
            break;
        }
        if !first {
            spans.push(Span::raw(sep.to_owned()));
        }
        spans.extend(item);
        used = used.saturating_add(need);
        first = false;
    }
    Line::from(spans)
}
