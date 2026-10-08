// SPDX-License-Identifier: MIT

use super::form_lines::form_lines;
use super::list_lines::list_lines;
use super::preview_lines::preview_lines;
use super::scroll::scroll_offset;
use crate::view::{NewSessionForm, ViewModel};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

/// Below this width the preview is dropped and the list gets the whole screen.
const MIN_WIDTH_FOR_PREVIEW: u16 = 80;
/// Width of the new-session form.
const FORM_WIDTH: u16 = 66;

/// Draw one frame. A pure function of the view model: no I/O, no state of its own.
pub fn render(frame: &mut Frame, vm: &ViewModel) {
    let [body, status] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    if body.width >= MIN_WIDTH_FOR_PREVIEW {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(body);
        draw_list(frame, vm, left);
        draw_preview(frame, vm, right);
    } else {
        draw_list(frame, vm, body);
    }
    draw_status(frame, vm, status);
    if let Some(form) = vm.form() {
        draw_form(frame, form, frame.area());
    }
}

/// The new-session form, centred over the dashboard.
fn draw_form(frame: &mut Frame, form: &NewSessionForm, area: ratatui::layout::Rect) {
    let lines = form_lines(form);
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, popup, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(FORM_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(middle);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" New session ")),
        popup,
    );
}

fn draw_list(frame: &mut Frame, vm: &ViewModel, area: ratatui::layout::Rect) {
    let block = Block::bordered().title(" Flight ");
    let inner = block.inner(area);
    let list = list_lines(vm);
    let offset = scroll_offset(
        list.selected_row,
        usize::from(inner.height),
        list.lines.len(),
    );
    let visible: Vec<Line> = list.lines.into_iter().skip(offset).collect();
    frame.render_widget(Paragraph::new(visible).block(block), area);
}

fn draw_preview(frame: &mut Frame, vm: &ViewModel, area: ratatui::layout::Rect) {
    let height = usize::from(area.height.saturating_sub(2));
    let (title, body) = preview_lines(vm, height);
    frame.render_widget(
        Paragraph::new(body).block(Block::bordered().title(format!(" {title} "))),
        area,
    );
}

fn draw_status(frame: &mut Frame, vm: &ViewModel, area: ratatui::layout::Rect) {
    let text = match vm.message() {
        Some(m) => m.to_owned(),
        None if !vm.loaded() => "loading…".to_owned(),
        None => {
            "↑/↓ move   Enter switch   n New session   Tab section   r refresh   q quit".to_owned()
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}
