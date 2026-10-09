// SPDX-License-Identifier: MIT

use super::style::dim;
use super::text::cells;
use crate::view::{Summary, ViewModel};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// `Flight` in the state colours, then the counts that matter, as many as fit.
pub fn header_line(vm: &ViewModel, width: usize) -> Line<'static> {
    let s = Summary::of(vm.snapshot());
    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    for (c, colour) in "Flight".chars().zip([
        Color::Red,
        Color::Yellow,
        Color::Green,
        Color::Cyan,
        Color::Blue,
        Color::Magenta,
    ]) {
        spans.push(Span::styled(
            c.to_string(),
            Style::default().fg(colour).add_modifier(Modifier::BOLD),
        ));
    }
    if !vm.loaded() {
        spans.push(Span::styled("   connecting…", dim()));
        return Line::from(spans);
    }
    let mut parts: Vec<Span<'static>> = Vec::new();
    let count = |n: usize, text: &str, style: Style| {
        (n > 0).then(|| Span::styled(format!("{n} {text}"), style))
    };
    let red = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    parts.extend(count(s.need_you, "need you", red));
    parts.extend(count(s.ready, "ready", Style::default().fg(Color::Green)));
    parts.extend(count(
        s.working,
        "working",
        Style::default().fg(Color::Yellow),
    ));
    parts.extend(count(s.idle, "idle", dim()));
    parts.extend(count(s.shell, "shell", dim()));
    parts.extend(count(s.unavailable, "saved unavailable", red));
    parts.extend(count(s.saved_stopped, "saved stopped", dim()));
    if s.workspaces() == 0 && s.unavailable == 0 && s.saved_stopped == 0 {
        parts.push(Span::styled("no workspaces", dim()));
    }
    parts.push(hosts_part(&s));
    spans.push(Span::styled("  ─  ", dim()));
    let mut used = spans.iter().map(Span::width).sum::<usize>();
    let mut first = true;
    for part in parts {
        let sep = if first { 0 } else { cells(" · ") };
        used = used.saturating_add(sep).saturating_add(part.width());
        if used > width {
            break;
        }
        if !first {
            spans.push(Span::styled(" · ", dim()));
        }
        spans.push(part);
        first = false;
    }
    Line::from(spans)
}

fn hosts_part(s: &Summary) -> Span<'static> {
    let noun = if s.hosts == 1 { "host" } else { "hosts" };
    if s.hosts_up < s.hosts {
        Span::styled(
            format!("{} of {} {noun} online", s.hosts_up, s.hosts),
            Style::default().fg(Color::Red),
        )
    } else {
        Span::styled(format!("{} {noun}", s.hosts), dim())
    }
}
