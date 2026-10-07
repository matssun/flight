// SPDX-License-Identifier: MIT

use super::style::state_look;
use crate::view::ViewModel;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

/// Title and body for the preview pane: the selected pane's identity and state, then the
/// bottom `height` lines of its screen.
pub fn preview_lines(vm: &ViewModel, height: usize) -> (String, Vec<Line<'static>>) {
    let Some(sel) = vm.selected() else {
        return ("Preview".to_owned(), vec![Line::raw("no pane selected")]);
    };
    let pane = vm
        .snapshot()
        .hosts
        .iter()
        .flat_map(|h| h.panes.iter())
        .find(|p| &p.pane_ref == sel);
    let title = match pane {
        Some(p) => {
            let (icon, label, _) = state_look(p.state);
            format!(
                "{} / {} · {:?} · {icon} {label} ({})",
                super::list_lines::host_label(vm, p).unwrap_or(""),
                p.session,
                p.agent,
                p.why
            )
        }
        None => "Preview".to_owned(),
    };
    let body = match vm.preview() {
        None => vec![Line::styled(
            "loading…",
            Style::default().add_modifier(Modifier::DIM),
        )],
        Some(prev) => match &prev.content {
            Ok(lines) => {
                let skip = lines.len().saturating_sub(height);
                lines
                    .iter()
                    .skip(skip)
                    .map(|l| Line::raw(l.clone()))
                    .collect()
            }
            Err(e) => vec![Line::raw(format!("cannot capture: {e}"))],
        },
    };
    (title, body)
}
