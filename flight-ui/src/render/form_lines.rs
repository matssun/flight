// SPDX-License-Identifier: MIT

//! The new-session form as plain lines. A pure function of the form.

use crate::view::{Field, NewSessionForm, Program};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const LABEL: usize = 11;

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
        (true, _) => Line::styled(
            "  Creating the session…",
            Style::default().add_modifier(Modifier::DIM),
        ),
        (false, Some(e)) => Line::styled(format!("  {e}"), Style::default().fg(Color::Red)),
        (false, None) => Line::raw(""),
    });
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        button(form, Field::Create, "Create"),
        Span::raw("   "),
        button(form, Field::Cancel, "Cancel"),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  Tab next   ←/→ change   Enter select   Esc cancel",
        Style::default().add_modifier(Modifier::DIM),
    ));
    lines
}

fn row(
    form: &NewSessionForm,
    field: Field,
    label: &str,
    value: Vec<Span<'static>>,
) -> Line<'static> {
    let focused = form.focus() == field;
    let marker = if focused { "▸ " } else { "  " };
    let style = if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut spans = vec![
        Span::styled(marker.to_owned(), style),
        Span::styled(format!("{:<LABEL$}", format!("{label}:")), style),
    ];
    spans.extend(value);
    Line::from(spans)
}

fn host_value(form: &NewSessionForm) -> Vec<Span<'static>> {
    match form.hosts().get(form.host_index()) {
        Some(h) => vec![Span::raw(format!("◂ {} ▸", h.label))],
        None => vec![Span::styled(
            "no node is connected",
            Style::default().fg(Color::Red),
        )],
    }
}

fn text_value(form: &NewSessionForm, field: Field) -> Vec<Span<'static>> {
    let (text, focused) = match field {
        Field::Name => (form.name(), form.focus() == Field::Name),
        _ => (form.dir(), form.focus() == Field::Directory),
    };
    let cursor = if focused { "▏" } else { "" };
    vec![Span::styled(
        format!("[{text}{cursor}]"),
        if focused {
            Style::default().add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        },
    )]
}

fn start_value(form: &NewSessionForm) -> Vec<Span<'static>> {
    let choice = |p: Program| {
        let on = form.program() == p;
        Span::styled(
            format!("({}) {}", if on { "•" } else { " " }, p.label()),
            if on {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::DIM)
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
            format!("[ {label} ]"),
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )
    } else {
        Span::raw(format!("[ {label} ]"))
    }
}
