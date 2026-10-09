// SPDX-License-Identifier: MIT

//! Saved workspaces that are not running: always listed, with the host, the configured root
//! and why they are unavailable. Presentation only; what is true of them is the node's report.

use super::list_view::ListView;
use super::style::{bold, dim, selected_row};
use super::text::{cells, fit, pad};
use crate::snapshot::{HostHealth, SavedHealth, SavedResume, SavedRoot, SavedView};
use crate::view::ViewModel;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// (icon, short reason, colour). The reason names the problem in the user's terms: the host
/// first (nothing else can be known about a host that cannot be asked), then the root.
pub(super) fn status(v: &SavedView) -> (&'static str, String, Color) {
    match &v.host_health {
        HostHealth::Online => {}
        HostHealth::Stale => return ("!", "host quiet · last known".to_owned(), Color::Yellow),
        _ => return ("!", "host unreachable · last known".to_owned(), Color::Red),
    }
    match (v.health, v.root_state) {
        (SavedHealth::Stopped, _) => ("○", "stopped".to_owned(), Color::DarkGray),
        (_, SavedRoot::Missing) => ("⚠", "directory missing".to_owned(), Color::Red),
        (_, SavedRoot::Unverified) => ("⚠", "directory unverified".to_owned(), Color::Yellow),
        (_, SavedRoot::PermissionDenied) => ("⚠", "no permission".to_owned(), Color::Red),
        (_, SavedRoot::NotADirectory) => ("⚠", "not a directory".to_owned(), Color::Red),
        (_, SavedRoot::Changed) => ("⚠", "directory changed".to_owned(), Color::Yellow),
        _ => ("⚠", "needs attention".to_owned(), Color::Red),
    }
}

pub(super) fn saved_rows(vm: &ViewModel, out: &mut ListView, width: usize, card: bool) {
    let saved = vm.unavailable();
    if saved.is_empty() {
        return;
    }
    if !out.lines.is_empty() {
        out.push(Line::raw(""), None);
    }
    let blocked = saved
        .iter()
        .filter(|v| v.health == SavedHealth::Blocked)
        .count();
    let head = if blocked > 0 {
        Color::Red
    } else {
        Color::DarkGray
    };
    out.push(
        Line::from(vec![
            Span::styled(" ▍", Style::default().fg(head)),
            Span::styled("SAVED · NOT RUNNING", bold().fg(head)),
            Span::styled(format!(" · {}", saved.len()), dim()),
        ]),
        None,
    );
    let name_w = saved
        .iter()
        .map(|v| cells(&v.name))
        .max()
        .unwrap_or(0)
        .clamp(6, 24);
    for v in &saved {
        let selected = vm.selected_key() == Some(&v.key());
        let start = out.lines.len();
        let (icon, reason, colour) = status(v);
        let mut row = vec![
            Span::styled(format!(" {icon} "), Style::default().fg(colour)),
            Span::styled(pad(&fit(&v.name, name_w), name_w), bold()),
            Span::styled(format!("  {}", v.host_label), dim()),
        ];
        if card {
            out.push(Line::from(row), None);
            out.push(
                Line::styled(
                    fit(&format!("   {reason}"), width),
                    Style::default().fg(colour),
                ),
                None,
            );
        } else {
            row.push(Span::styled(
                format!("  {reason}"),
                Style::default().fg(colour),
            ));
            out.push(Line::from(row), None);
        }
        if selected {
            for l in details(v, width) {
                out.push(l, None);
            }
            out.selected = Some((start, out.lines.len().saturating_sub(1)));
            for line in out.lines.iter_mut().skip(start) {
                *line = line.clone().patch_style(selected_row());
            }
        }
    }
}

/// What the user needs to see to act: where, what Flight found, and what it did not do.
fn details(v: &SavedView, width: usize) -> Vec<Line<'static>> {
    let row = |label: &str, text: String| {
        Line::from(vec![
            Span::styled(format!("    {label:<7}"), dim()),
            Span::raw(fit(&text, width.saturating_sub(12))),
        ])
    };
    let mut lines = vec![
        row(
            "host",
            format!("{} ({})", v.host_label, host_word(&v.host_health)),
        ),
        row("root", v.root.clone()),
    ];
    if !v.detail.is_empty() {
        lines.push(row("found", v.detail.clone()));
    }
    let (headline, why, hint) = agent_lines(v);
    if let Some(headline) = headline {
        lines.push(row("agent", headline));
        for part in wrap(&why, width.saturating_sub(12)) {
            lines.push(Line::styled(format!("            {part}"), dim()));
        }
        if let Some(hint) = hint {
            lines.push(Line::styled(format!("            {hint}"), dim()));
        }
    }
    lines.push(Line::styled(
        fit(
            "    Kept as saved. Flight does not create or repair directories.",
            width,
        ),
        dim(),
    ));
    lines
}

/// What starting it again does about the agent's conversation, in the words the node's report
/// supports and no stronger: a headline, the node's reason (if any, to be wrapped) and a hint.
fn agent_lines(v: &SavedView) -> (Option<String>, String, Option<&'static str>) {
    if v.health != SavedHealth::Stopped {
        return (None, String::new(), None);
    }
    match &v.resume {
        SavedResume::Unknown => (None, String::new(), None),
        SavedResume::New => (
            Some("Enter starts a new conversation (none was saved)".to_owned()),
            String::new(),
            None,
        ),
        SavedResume::Continues => (
            Some("Enter continues the earlier conversation".to_owned()),
            String::new(),
            Some("f starts a new conversation instead"),
        ),
        SavedResume::CannotContinue(why) => (
            Some("cannot continue the earlier conversation".to_owned()),
            why.clone(),
            Some("f starts a new conversation instead"),
        ),
        SavedResume::Unsupported(why) => (
            Some("Enter starts a new conversation".to_owned()),
            why.clone(),
            None,
        ),
    }
}

/// `text` in lines of at most `width` cells, broken at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(last) if cells(last) + 1 + cells(word) <= width => {
                last.push(' ');
                last.push_str(word);
            }
            _ => lines.push(fit(word, width.max(1))),
        }
    }
    lines
}

fn host_word(h: &HostHealth) -> &'static str {
    match h {
        HostHealth::Online => "online",
        HostHealth::Stale => "quiet lately",
        _ => "unreachable",
    }
}

/// The preview pane for a selected saved workspace: the same facts, with room.
pub(super) fn saved_preview(v: &SavedView, width: usize) -> Vec<Line<'static>> {
    let (icon, reason, colour) = status(v);
    let mut out = vec![
        Line::from(vec![
            Span::styled(format!(" {icon} "), Style::default().fg(colour)),
            Span::styled(v.name.clone(), bold()),
            Span::styled(
                format!("  {}", reason.to_uppercase()),
                Style::default().fg(colour),
            ),
        ]),
        Line::styled(
            format!(" saved workspace on {} · not running", v.host_label),
            dim(),
        ),
        Line::styled("─".repeat(width), dim()),
    ];
    out.extend(details(v, width));
    out.push(Line::raw(""));
    out.push(Line::styled(
        fit(
            " The host re-checks every few seconds; it comes back by itself when the directory or host does.",
            width,
        ),
        dim(),
    ));
    if v.imported {
        out.push(Line::styled(
            fit(" Imported: it starts nothing until you allow it.", width),
            dim(),
        ));
    }
    out
}
