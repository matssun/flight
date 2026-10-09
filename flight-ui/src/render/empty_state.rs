// SPDX-License-Identifier: MIT

use super::style::{bold, dim, key};
use crate::snapshot::HostHealth;
use crate::view::{Summary, ViewModel};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// What the list says when there is nothing to list, and why.
pub fn empty_state(vm: &ViewModel) -> Vec<Line<'static>> {
    let mut out = vec![Line::raw("")];
    if !vm.loaded() {
        out.push(Line::styled("  Connecting…", dim()));
        return out;
    }
    if !vm.filter().trim().is_empty() {
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("No workspaces match “{}”", vm.filter()), bold()),
        ]));
        out.push(Line::raw(""));
        out.push(hint("  ", &[("Esc", " clears the search")]));
        return out;
    }
    let s = Summary::of(vm.snapshot());
    if s.hosts_up == 0 {
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No host is connected", bold().fg(Color::Red)),
        ]));
        out.push(Line::raw(""));
        out.push(Line::styled(
            "  Flight is waiting for a machine to report in.",
            dim(),
        ));
        out.push(Line::styled(
            "  Check that Flight is running on your hosts.",
            dim(),
        ));
        return out;
    }
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("●", Style::default().fg(Color::Green)),
        Span::styled(" No Flight workspaces yet", bold()),
    ]));
    out.push(Line::raw(""));
    out.push(hint("  Press ", &[("n", " to start a workspace")]));
    out.push(Line::styled("  on one of your connected hosts.", dim()));
    out.push(Line::raw(""));
    out.push(Line::styled("  Connected hosts", dim()));
    for h in vm
        .snapshot()
        .hosts
        .iter()
        .filter(|h| matches!(h.health, HostHealth::Online | HostHealth::NoServer))
    {
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("● ", Style::default().fg(Color::Green)),
            Span::styled(h.label.clone(), bold()),
        ]));
    }
    out
}

fn hint(lead: &str, parts: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(lead.to_owned())];
    for (k, rest) in parts {
        if !k.is_empty() {
            spans.push(Span::styled((*k).to_owned(), key()));
        }
        spans.push(Span::styled((*rest).to_owned(), dim()));
    }
    Line::from(spans)
}
