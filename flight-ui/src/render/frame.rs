// SPDX-License-Identifier: MIT

use super::list_lines::list_lines;
use super::preview_lines::preview_lines;
use super::scroll::scroll_offset;
use crate::view::ViewModel;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Below this width the preview is dropped and the list gets the whole screen.
const MIN_WIDTH_FOR_PREVIEW: u16 = 80;

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
        None => "↑/↓ move   Enter switch   Tab section   r refresh   q quit".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}
