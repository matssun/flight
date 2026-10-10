// SPDX-License-Identifier: MIT

use super::render;
use crate::view::ViewModel;
use ratatui::backend::TestBackend;
use ratatui::text::Span;
use ratatui::Terminal;

/// Render one frame to plain text (rows joined by newlines, trailing spaces trimmed). Used by
/// `flight --once` and by tests, so the real renderer is exercised without a terminal.
pub fn render_to_string(vm: &ViewModel, width: u16, height: u16) -> String {
    // The test backend cannot fail (its error type is `Infallible`).
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height));
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

/// Display width of `s` in terminal cells.
pub fn cells(s: &str) -> usize {
    Span::raw(s.to_owned()).width()
}

/// `s` cut to at most `width` cells, with `…` where it was cut.
pub fn fit(s: &str, width: usize) -> String {
    if cells(s) <= width {
        return s.to_owned();
    }
    let mut out = String::new();
    for c in s.chars() {
        let next = format!("{out}{c}");
        if cells(&next).saturating_add(1) > width {
            break;
        }
        out = next;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// The end of `s` that fits in `width` cells, with `…` where the start was cut: what a text
/// field shows while it is being typed into, so the caret stays in view.
pub fn fit_tail(s: &str, width: usize) -> String {
    if cells(s) <= width {
        return s.to_owned();
    }
    let mut out = String::new();
    for c in s.chars().rev() {
        let next = format!("{c}{out}");
        if cells(&next).saturating_add(1) > width {
            break;
        }
        out = next;
    }
    format!("…{out}")
}

/// `s` fit to `width` and padded with spaces to exactly `width` cells.
pub fn pad(s: &str, width: usize) -> String {
    let t = fit(s, width);
    let gap = width.saturating_sub(cells(&t));
    format!("{t}{}", " ".repeat(gap))
}
