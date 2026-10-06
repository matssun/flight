// SPDX-License-Identifier: MIT

use super::render;
use crate::view::ViewModel;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Render one frame to plain text (rows joined by newlines, trailing spaces trimmed). Used by
/// `flight --once` and by tests, so the real renderer is exercised without a terminal.
pub fn render_to_string(vm: &ViewModel, width: u16, height: u16) -> String {
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height)) else {
        return String::new();
    };
    if terminal.draw(|f| render(f, vm)).is_err() {
        return String::new();
    }
    let buf = terminal.backend().buffer().clone();
    let w = usize::from(width);
    buf.content()
        .chunks(w.max(1))
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
