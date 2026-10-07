// SPDX-License-Identifier: MIT

//! The left column as plain lines: ATTENTION, then HOSTS. A pure function of the view model.

use super::style::{health_look, state_look};
use crate::snapshot::PaneView;
use crate::view::{attention_panes, Section, ViewModel};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// The lines, and the row the selection is on (for scrolling).
pub struct ListLines {
    pub lines: Vec<Line<'static>>,
    pub selected_row: Option<usize>,
}

pub fn list_lines(vm: &ViewModel) -> ListLines {
    let mut out = ListLines {
        lines: Vec::new(),
        selected_row: None,
    };
    heading(&mut out, "ATTENTION");
    let attention = attention_panes(vm.snapshot());
    if attention.is_empty() {
        out.lines.push(Line::styled(
            "  nothing needs you",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    for p in attention {
        let marker = marker(vm, p, Section::Attention);
        push_pane(&mut out, p, marker, host_label(vm, p));
    }
    out.lines.push(Line::raw(""));
    heading(&mut out, "HOSTS");
    for h in &vm.snapshot().hosts {
        let (icon, text, colour) = health_look(&h.health);
        out.lines.push(Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(colour)),
            Span::styled(
                h.label.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {text}"), Style::default().fg(colour)),
        ]));
        for p in &h.panes {
            let marker = marker(vm, p, Section::Tree);
            push_pane(&mut out, p, marker, None);
        }
    }
    out
}

fn heading(out: &mut ListLines, text: &'static str) {
    out.lines.push(Line::styled(
        text,
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ));
}

/// `>` where the cursor is, `·` for the same pane in the section without focus.
fn marker(vm: &ViewModel, p: &PaneView, section: Section) -> &'static str {
    if vm.selected() != Some(&p.pane_ref) {
        "  "
    } else if vm.focus() == section {
        "> "
    } else {
        "· "
    }
}

/// The display label of the host a pane lives on.
pub(super) fn host_label<'a>(vm: &'a ViewModel, p: &'a PaneView) -> Option<&'a str> {
    Some(
        vm.snapshot()
            .hosts
            .iter()
            .find(|h| h.host == p.pane_ref.host)
            .map_or(p.pane_ref.host.as_str(), |h| h.label.as_str()),
    )
}

fn push_pane(out: &mut ListLines, p: &PaneView, marker: &'static str, host: Option<&str>) {
    let (icon, label, colour) = state_look(p.state);
    if marker == "> " {
        out.selected_row = Some(out.lines.len());
    }
    let mut spans = vec![Span::raw(if host.is_some() {
        marker.to_owned()
    } else {
        format!("  {marker}")
    })];
    if let Some(label) = host {
        spans.push(Span::styled(
            format!("{label:<9.9}"),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    spans.push(Span::raw(format!("{:<16}", p.session)));
    spans.push(Span::raw(format!("{:<9}", format!("{:?}", p.agent))));
    spans.push(Span::styled(
        format!("{icon} {label}"),
        Style::default().fg(colour),
    ));
    let mut line = Line::from(spans);
    if marker == "> " {
        line = line.style(Style::default().add_modifier(Modifier::REVERSED));
    }
    out.lines.push(line);
}
