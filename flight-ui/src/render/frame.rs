// SPDX-License-Identifier: MIT

use super::footer::{hints_line, legend_line, message_line};
use super::form_lines::form_lines;
use super::header::header_line;
use super::help::help_lines;
use super::layout::{areas, Areas, CARD_BELOW, MIN_HEIGHT, MIN_WIDTH};
use super::list_view::{list_view, ListView};
use super::preview_view::preview_view;
use super::prompt_lines::prompt_lines;
use super::saved_prompt_lines::{saved_prompt_lines, title as saved_title};
use super::scroll::scroll_offset;
use super::style::dim;
use crate::view::{NewSessionForm, SavedPrompt, ShellPrompt, ViewModel};
use flight_state::PaneRef;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

/// Width of the new-workspace form.
const FORM_WIDTH: u16 = 72;
/// Width of the help overlay.
const HELP_WIDTH: u16 = 68;

/// Draw one frame. A pure function of the view model: no I/O, no state of its own.
pub fn render(frame: &mut Frame, vm: &ViewModel) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(Paragraph::new("Terminal too small").style(dim()), area);
        return;
    }
    let a = areas(area);
    frame.render_widget(
        Paragraph::new(header_line(vm, usize::from(a.header.width))),
        a.header,
    );
    draw_list(frame, vm, &a);
    if let Some(preview) = a.preview {
        draw_preview(frame, vm, preview);
    }
    draw_footer(frame, vm, &a);
    if let Some(form) = vm.form() {
        draw_form(frame, form, area);
    } else if let Some(prompt) = vm.saved_prompt() {
        draw_saved_prompt(frame, prompt, area);
    } else if let Some(prompt) = vm.prompt() {
        draw_prompt(frame, prompt, area);
    } else if vm.help_open() {
        draw_help(frame, area);
    }
}

fn pane_block(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(format!(" {title} "))
}

/// The list's lines for this area, and the first one on screen.
fn list_for(vm: &ViewModel, list_area: Rect) -> (ListView, usize, Rect) {
    let inner = pane_block("Workspaces").inner(list_area);
    let card = inner.width < CARD_BELOW;
    let view = list_view(vm, usize::from(inner.width), card);
    let offset = scroll_offset(view.selected, usize::from(inner.height), view.lines.len());
    (view, offset, inner)
}

fn draw_list(frame: &mut Frame, vm: &ViewModel, a: &Areas) {
    let (view, offset, _) = list_for(vm, a.list);
    let visible: Vec<Line> = view.lines.into_iter().skip(offset).collect();
    frame.render_widget(
        Paragraph::new(visible).block(pane_block("Workspaces")),
        a.list,
    );
}

fn draw_preview(frame: &mut Frame, vm: &ViewModel, area: Rect) {
    let block = pane_block("Preview");
    let inner = block.inner(area);
    let body = preview_view(vm, usize::from(inner.height), usize::from(inner.width));
    frame.render_widget(Paragraph::new(body).block(block), area);
}

fn draw_footer(frame: &mut Frame, vm: &ViewModel, a: &Areas) {
    let width = usize::from(a.footer.width);
    let hints = hints_line(vm, width);
    let lines = if a.footer.height >= 2 {
        let first = match vm.message() {
            Some(m) => message_line(m),
            None if !vm.loaded() => message_line("loading…"),
            None => legend_line(width),
        };
        vec![first, hints]
    } else {
        vec![match vm.message() {
            Some(m) => message_line(m),
            None => hints,
        }]
    };
    frame.render_widget(Paragraph::new(lines), a.footer);
}

/// A bordered box of `lines` centred in `area`.
fn modal(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    width: u16,
    lines: Vec<Line<'static>>,
) {
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, popup, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(middle);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan))
                .title(format!(" {title} ")),
        ),
        popup,
    );
}

fn draw_form(frame: &mut Frame, form: &NewSessionForm, area: Rect) {
    modal(frame, area, "New workspace", FORM_WIDTH, form_lines(form));
}

fn draw_prompt(frame: &mut Frame, prompt: &ShellPrompt, area: Rect) {
    modal(
        frame,
        area,
        "Companion shell",
        FORM_WIDTH,
        prompt_lines(prompt),
    );
}

fn draw_saved_prompt(frame: &mut Frame, prompt: &SavedPrompt, area: Rect) {
    modal(
        frame,
        area,
        saved_title(prompt),
        FORM_WIDTH,
        saved_prompt_lines(prompt),
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    modal(frame, area, "Help", HELP_WIDTH, help_lines());
}

/// The session drawn at screen cell (`x`, `y`), if any: what a click there means.
pub fn session_at(vm: &ViewModel, area: Rect, x: u16, y: u16) -> Option<PaneRef> {
    if area.width < MIN_WIDTH
        || area.height < MIN_HEIGHT
        || vm.form().is_some()
        || vm.prompt().is_some()
        || vm.saved_prompt().is_some()
        || vm.help_open()
    {
        return None;
    }
    let a = areas(area);
    let (view, offset, inner) = list_for(vm, a.list);
    let inside = x >= inner.x
        && x < inner.x.saturating_add(inner.width)
        && y >= inner.y
        && y < inner.y.saturating_add(inner.height);
    if !inside {
        return None;
    }
    let line = usize::from(y.saturating_sub(inner.y)).saturating_add(offset);
    view.panes.get(line).cloned().flatten()
}
