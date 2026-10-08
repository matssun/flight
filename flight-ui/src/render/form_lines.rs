// SPDX-License-Identifier: MIT

//! The new-session form as plain lines. A pure function of the form.

use super::style::{dim, key};
use super::text::{fit_tail, pad};
use crate::view::{Field, NewSessionForm, Program};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const LABEL: usize = 11;
const FIELD: usize = 40;

/// The form's lines: one row per field, any error, the buttons, and a line of key help.
pub fn form_lines(form: &NewSessionForm) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw("")];
    lines.push(row(form, Field::Host, "Host", host_value(form)));
    lines.push(row(
        form,
        Field::Name,
        "Name",
        text_value(form, Field::Name),
    ));
    lines.push(row(
        form,
        Field::Directory,
        "Directory",
        text_value(form, Field::Directory),
    ));
    lines.push(row(form, Field::Start, "Start", start_value(form)));
    lines.push(Line::raw(""));
    lines.push(match (form.submitting(), form.error()) {
        (true, _) => Line::styled("  Creating the session…", dim()),
        (false, Some(e)) => Line::styled(format!("  {e}"), Style::default().fg(Color::Red)),
        (false, None) => Line::styled("  The directory is on the chosen host.", dim()),
    });
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(LABEL.saturating_add(4))),
        button(form, Field::Create, "Create"),
        Span::raw("  "),
        button(form, Field::Cancel, "Cancel"),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  Tab", key()),
        Span::styled(" next   ", dim()),
        Span::styled("←/→", key()),
        Span::styled(" choose   ", dim()),
        Span::styled("Enter", key()),
        Span::styled(" confirm   ", dim()),
        Span::styled("Esc", key()),
        Span::styled(" cancel", dim()),
    ]));
    lines
}

fn row(
    form: &NewSessionForm,
    field: Field,
    label: &str,
    value: Vec<Span<'static>>,
) -> Line<'static> {
    let focused = form.focus() == field;
    let marker = if focused { "▌ " } else { "  " };
    let label_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut spans = vec![
        Span::styled(marker.to_owned(), Style::default().fg(Color::Cyan)),
        Span::styled(format!("{label:<LABEL$}"), label_style),
    ];
    spans.extend(value);
    Line::from(spans)
}

/// A bracketed box of fixed width, brighter when focused.
fn boxed(text: &str, focused: bool) -> Span<'static> {
    let style = if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Span::styled(format!("[{}]", pad(text, FIELD)), style)
}

fn host_value(form: &NewSessionForm) -> Vec<Span<'static>> {
    match form.hosts().get(form.host_index()) {
        Some(h) => {
            let arrows = if form.hosts().len() > 1 {
                "◂ ▸"
            } else {
                "   "
            };
            let text = format!("{}  {arrows}", h.label);
            vec![boxed(&text, form.focus() == Field::Host)]
        }
        None => vec![Span::styled(
            "no host is connected",
            Style::default().fg(Color::Red),
        )],
    }
}

fn text_value(form: &NewSessionForm, field: Field) -> Vec<Span<'static>> {
    let (text, focused) = match field {
        Field::Name => (form.name(), form.focus() == Field::Name),
        _ => (form.dir(), form.focus() == Field::Directory),
    };
    let shown = if focused {
        fit_tail(&format!("{text}▏"), FIELD)
    } else {
        text.to_owned()
    };
    vec![boxed(&shown, focused)]
}

fn start_value(form: &NewSessionForm) -> Vec<Span<'static>> {
    let choice = |p: Program| {
        let on = form.program() == p;
        Span::styled(
            format!("({}) {}", if on { "•" } else { " " }, p.label()),
            if on {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                dim()
            },
        )
    };
    vec![
        choice(Program::Claude),
        Span::raw("   "),
        choice(Program::Shell),
    ]
}

fn button(form: &NewSessionForm, field: Field, label: &str) -> Span<'static> {
    if form.focus() == field {
        Span::styled(
            format!(" {label} "),
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {label} "), Style::default().fg(Color::Cyan))
    }
}
