// SPDX-License-Identifier: MIT

//! The session list as lines: sessions grouped by how much they want the user, most urgent
//! first, then any host that is not healthy. A pure function of the view model and a width.

use super::empty_state::empty_state;
use super::style::{bold, dim, health_look, selected_row, state_icon, state_look, tier_colour};
use super::text::{cells, fit, pad};
use crate::snapshot::{HostHealth, PaneView};
use crate::view::{Tier, ViewModel};
use flight_state::PaneRef;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The lines, which session each belongs to, and where the selection is.
pub struct ListView {
    pub lines: Vec<Line<'static>>,
    /// One entry per line: the session it shows, if it shows one.
    pub panes: Vec<Option<PaneRef>>,
    /// First and last line of the selected session.
    pub selected: Option<(usize, usize)>,
}

impl ListView {
    fn push(&mut self, line: Line<'static>, pane: Option<PaneRef>) {
        self.lines.push(line);
        self.panes.push(pane);
    }
}

const STATE_W: usize = 8;
const AGENT_W: usize = 8;

pub fn list_view(vm: &ViewModel, width: usize, card: bool) -> ListView {
    let mut out = ListView {
        lines: Vec::new(),
        panes: Vec::new(),
        selected: None,
    };
    let listed = vm.listed();
    if listed.is_empty() {
        for l in empty_state(vm) {
            out.push(l, None);
        }
    }
    let name_w = listed
        .iter()
        .map(|p| cells(&p.session))
        .max()
        .unwrap_or(0)
        .clamp(6, 24);
    let host_w = listed
        .iter()
        .map(|p| cells(host_label(vm, p)))
        .max()
        .unwrap_or(0)
        .min(14);
    let mut tier: Option<Tier> = None;
    for p in &listed {
        let t = Tier::of(p.state);
        if tier != Some(t) {
            if tier.is_some() {
                out.push(Line::raw(""), None);
            }
            let n = listed.iter().filter(|q| Tier::of(q.state) == t).count();
            out.push(tier_header(t, n), None);
            tier = Some(t);
        }
        let selected = vm.selected() == Some(&p.pane_ref);
        let start = out.lines.len();
        let ctx = Row {
            vm,
            p,
            width,
            name_w,
            host_w,
            selected,
        };
        if card {
            let (a, b) = ctx.card();
            out.push(a, Some(p.pane_ref.clone()));
            out.push(b, Some(p.pane_ref.clone()));
        } else {
            out.push(ctx.line(), Some(p.pane_ref.clone()));
        }
        if selected {
            out.selected = Some((start, out.lines.len().saturating_sub(1)));
        }
    }
    host_problems(vm, &mut out);
    out
}

fn tier_header(t: Tier, n: usize) -> Line<'static> {
    let c = tier_colour(t);
    Line::from(vec![
        Span::styled(" ▍", Style::default().fg(c)),
        Span::styled(
            t.label().to_uppercase(),
            Style::default().fg(c).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" · {n}"), dim()),
    ])
}

/// Hosts that are not healthy, so a missing session can be explained.
fn host_problems(vm: &ViewModel, out: &mut ListView) {
    let bad: Vec<_> = vm
        .snapshot()
        .hosts
        .iter()
        .filter(|h| !matches!(h.health, HostHealth::Online | HostHealth::NoServer))
        .collect();
    if bad.is_empty() {
        return;
    }
    out.push(Line::raw(""), None);
    out.push(Line::styled(" ▍HOSTS", bold().fg(Color::Red)), None);
    for h in bad {
        let (icon, text, colour) = health_look(&h.health);
        out.push(
            Line::from(vec![
                Span::styled(format!("  {icon} "), Style::default().fg(colour)),
                Span::styled(h.label.clone(), bold()),
                Span::styled(format!("  {text}"), Style::default().fg(colour)),
            ]),
            None,
        );
    }
}

/// The display name of the host a session lives on.
pub(super) fn host_label<'a>(vm: &'a ViewModel, p: &'a PaneView) -> &'a str {
    vm.snapshot()
        .hosts
        .iter()
        .find(|h| h.host == p.pane_ref.host)
        .map_or(p.pane_ref.host.as_str(), |h| h.label.as_str())
}

/// The agent's name as the user knows it.
pub(super) fn agent_name(p: &PaneView) -> String {
    format!("{:?}", p.agent).to_lowercase()
}

struct Row<'a> {
    vm: &'a ViewModel,
    p: &'a PaneView,
    width: usize,
    name_w: usize,
    host_w: usize,
    selected: bool,
}

impl Row<'_> {
    fn colour(&self) -> Color {
        state_look(self.p.state).2
    }

    fn bar(&self) -> Span<'static> {
        if self.selected {
            Span::styled("▌", Style::default().fg(self.colour()))
        } else {
            Span::raw(" ")
        }
    }

    /// The name is the row's identity: bold, in the state's colour when the state matters.
    fn name_style(&self) -> Style {
        let base = Style::default().add_modifier(Modifier::BOLD);
        match self.colour() {
            Color::DarkGray => base,
            c => base.fg(c),
        }
    }

    /// The selected row is highlighted edge to edge, however short its text.
    fn finish(&self, mut line: Line<'static>) -> Line<'static> {
        if !self.selected {
            return line;
        }
        let gap = self.width.saturating_sub(line.width());
        if gap > 0 {
            line.spans.push(Span::raw(" ".repeat(gap)));
        }
        line.style(selected_row())
    }

    /// `▌ ⠋ name          working  claude        dev1`
    fn line(&self) -> Line<'static> {
        let (_, label, colour) = state_look(self.p.state);
        let icon = state_icon(self.p.state, self.vm.spinner_frame());
        let host = host_label(self.vm, self.p);
        // bar, space, icon, space
        let room = self.width.saturating_sub(4);
        let host_w = self.host_w;
        let full = self
            .name_w
            .saturating_add(1 + STATE_W + 1 + AGENT_W + 1)
            .saturating_add(host_w);
        let without_agent = self
            .name_w
            .saturating_add(1 + STATE_W + 1)
            .saturating_add(host_w);
        let show_agent = room >= full;
        let (name_w, show_host) = if show_agent || room >= without_agent {
            (self.name_w, true)
        } else {
            // Shrink the name before dropping the host: where it lives matters more.
            let need = 1 + STATE_W + 1 + host_w;
            let name_w = room.saturating_sub(need);
            if name_w >= 6 {
                (name_w, true)
            } else {
                (room.saturating_sub(1 + STATE_W).max(1), false)
            }
        };
        let mut spans = vec![
            self.bar(),
            Span::raw(" "),
            Span::styled(icon.to_owned(), Style::default().fg(colour)),
            Span::raw(" "),
            Span::styled(pad(&self.p.session, name_w), self.name_style()),
            Span::raw(" "),
            Span::styled(pad(label, STATE_W), Style::default().fg(colour)),
        ];
        let mut used = 4usize
            .saturating_add(name_w)
            .saturating_add(1)
            .saturating_add(STATE_W);
        if show_agent {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(pad(&agent_name(self.p), AGENT_W), dim()));
            used = used.saturating_add(1 + AGENT_W);
        }
        if show_host {
            // Right-aligned, so hosts line up down the right edge.
            let h = fit(host, host_w);
            let gap = self
                .width
                .saturating_sub(used)
                .saturating_sub(cells(&h))
                .max(1);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(h, dim()));
        }
        self.finish(Line::from(spans))
    }

    /// Two lines for a narrow list: the name, then what it is doing and where.
    fn card(&self) -> (Line<'static>, Line<'static>) {
        let (_, label, colour) = state_look(self.p.state);
        let icon = state_icon(self.p.state, self.vm.spinner_frame());
        let room = self.width.saturating_sub(4);
        let first = Line::from(vec![
            self.bar(),
            Span::raw(" "),
            Span::styled(icon.to_owned(), Style::default().fg(colour)),
            Span::raw(" "),
            Span::styled(fit(&self.p.session, room), self.name_style()),
        ]);
        let detail = format!("{label} · {}", host_label(self.vm, self.p));
        let second = Line::from(vec![
            self.bar(),
            Span::raw("   "),
            Span::styled(fit(&detail, room), dim()),
        ]);
        (self.finish(first), self.finish(second))
    }
}
