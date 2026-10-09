// SPDX-License-Identifier: MIT

use super::list_view::agent_name;
use super::style::{bold, dim, state_icon, state_look, state_meaning};
use super::text::fit;
use crate::snapshot::Workspace;
use crate::view::ViewModel;
use flight_state::AgentState;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// The preview pane's content: what the selected session is and does, its recent screen, and
/// what Enter will do. `height` is the rows available inside the pane.
pub fn preview_view(vm: &ViewModel, height: usize, width: usize) -> Vec<Line<'static>> {
    let Some(w) = vm.selected_workspace() else {
        return vec![
            Line::raw(""),
            Line::styled("  Select a workspace to see what it is doing.", dim()),
        ];
    };
    let Some(p) = w.anchor_pane() else {
        return vec![Line::raw("")];
    };
    let (_, label, colour) = state_look(p.state);
    let mut out = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled(
                state_icon(p.state, vm.spinner_frame()).to_owned(),
                Style::default().fg(colour),
            ),
            Span::raw(" "),
            Span::styled(w.name.clone(), bold()),
            Span::styled(
                format!("  {}", label.to_uppercase()),
                Style::default().fg(colour),
            ),
        ]),
        Line::styled(
            format!(
                " {} on {} · {}",
                agent_name(p),
                w.host_label,
                state_meaning(p.state)
            ),
            dim(),
        ),
        Line::styled(fit(&surfaces_summary(&w), width), dim()),
        Line::styled("─".repeat(width), dim()),
    ];
    let action = action_hint(p.state);
    // Rows left for the screen: after the header above, and the rule and hint below.
    let room = height.saturating_sub(out.len()).saturating_sub(3);
    match vm.preview().map(|prev| &prev.content) {
        None => out.push(Line::styled(" Loading preview…", dim())),
        Some(Err(_)) => out.push(Line::styled(
            " The screen is not available right now.",
            dim(),
        )),
        Some(Ok(lines)) => {
            let shown: Vec<&String> = {
                // Trailing blank rows are the empty bottom of the screen, not content.
                let end = lines
                    .iter()
                    .rposition(|l| !l.trim().is_empty())
                    .map_or(0, |i| i.saturating_add(1));
                let start = end.saturating_sub(room);
                lines.iter().take(end).skip(start).collect()
            };
            for l in shown {
                out.push(Line::raw(format!(" {l}")));
            }
        }
    }
    // The hints sit at the bottom of the pane.
    while out.len() < height.saturating_sub(3) {
        out.push(Line::raw(""));
    }
    out.truncate(height.saturating_sub(3));
    out.push(Line::styled("─".repeat(width), dim()));
    out.push(Line::styled(
        format!(" {action}"),
        Style::default().fg(accent(p.state)),
    ));
    if p.state != AgentState::Down {
        out.push(Line::styled(
            " In a session: Ctrl-Space a agent · s shell · q back here.",
            dim(),
        ));
    }
    out
}

/// `~/dev/nga · shell ready`: where the surfaces start and whether a shell is there yet.
fn surfaces_summary(w: &Workspace) -> String {
    let shell = match w.shell() {
        Some(_) => "shell ready",
        None => "no shell yet",
    };
    format!(" {} · {shell}", w.root)
}

fn accent(state: AgentState) -> Color {
    state_look(state).2
}

/// What the user can do about this session, in a sentence.
fn action_hint(state: AgentState) -> &'static str {
    match state {
        AgentState::Permit => "Needs your approval. Enter opens it so you can answer.",
        AgentState::Question => "Asked you something. Enter opens it so you can answer.",
        AgentState::Done => "Finished. Enter opens it for your next move.",
        AgentState::Busy => "Working. Enter opens it to watch or step in.",
        AgentState::Idle => "Idle. Enter opens it.",
        AgentState::Shell => "A shell. Enter opens it.",
        AgentState::Down => "Nothing is running here.",
    }
}
