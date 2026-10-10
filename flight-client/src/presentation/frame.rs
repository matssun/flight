// SPDX-License-Identifier: MIT

use crate::screens::{MouseMode, Painted};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Where the backend writes: a buffer this frame also writes its own escapes to, in order.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut out) = self.0.lock() {
            out.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Turns successive pictures of the whole terminal into the bytes that change the real one from
/// each to the next, and keeps the real terminal's cursor and key-reporting modes in step with
/// the surface that has the keyboard.
pub(super) struct Frame {
    shown: Buffer,
    backend: CrosstermBackend<Sink>,
    sink: Sink,
    applied: Option<(bool, bool)>,
    pointer: Option<MouseMode>,
    cursor_visible: bool,
}

impl Frame {
    pub(super) fn new(cols: u16, rows: u16) -> Self {
        let sink = Sink::default();
        Self {
            shown: Buffer::empty(Rect::new(0, 0, cols, rows)),
            backend: CrosstermBackend::new(sink.clone()),
            sink,
            applied: None,
            pointer: None,
            cursor_visible: true,
        }
    }

    /// A buffer of the terminal's size to paint into. A change of size forgets what was shown
    /// and clears the terminal, so the next picture is drawn in full.
    pub(super) fn blank(&mut self, cols: u16, rows: u16) -> Buffer {
        let area = Rect::new(0, 0, cols, rows);
        if self.shown.area != area {
            // A cleared terminal is all blanks, which is what an empty buffer is.
            self.shown = Buffer::empty(area);
            let _ = self.sink.write_all(b"\x1b[2J");
        }
        Buffer::empty(area)
    }

    /// The bytes that bring the terminal to `next`, with the cursor and the modes `painted`
    /// says (those of the surface with the keyboard).
    pub(super) fn bytes(&mut self, next: Buffer, painted: Painted) -> Vec<u8> {
        let updates = self.shown.diff(&next);
        if !updates.is_empty() {
            self.hide_cursor();
            let _ = self.backend.draw(updates.into_iter());
        }
        self.shown = next;
        match painted.cursor {
            Some((col, row)) => {
                let _ = self.backend.set_cursor_position(Position::new(col, row));
                self.show_cursor();
            }
            None => self.hide_cursor(),
        }
        if let Some(modes) = painted.modes {
            let want = (modes.bracketed_paste, modes.application_cursor_keys);
            if self.applied != Some(want) {
                let _ = self.sink.write_all(mode(2004, want.0).as_bytes());
                let _ = self.sink.write_all(mode(1, want.1).as_bytes());
                self.applied = Some(want);
            }
        }
        if self.pointer != Some(painted.pointer) {
            let _ = self.sink.write_all(pointer(painted.pointer).as_bytes());
            self.pointer = Some(painted.pointer);
        }
        let _ = Backend::flush(&mut self.backend);
        self.sink
            .0
            .lock()
            .map(|mut out| std::mem::take(&mut *out))
            .unwrap_or_default()
    }

    fn hide_cursor(&mut self) {
        if self.cursor_visible {
            let _ = self.sink.write_all(b"\x1b[?25l");
            self.cursor_visible = false;
        }
    }

    fn show_cursor(&mut self) {
        if !self.cursor_visible {
            let _ = self.sink.write_all(b"\x1b[?25h");
            self.cursor_visible = true;
        }
    }
}

fn mode(number: u16, on: bool) -> String {
    format!("\x1b[?{number}{}", if on { 'h' } else { 'l' })
}

/// Switch the real terminal's mouse reporting to `wanted`: the one level, in the SGR form (the
/// only one with no limit on position that Flight reads), or all off.
fn pointer(wanted: MouseMode) -> String {
    let level = match wanted {
        MouseMode::None => None,
        MouseMode::Press | MouseMode::PressRelease => Some(1000),
        MouseMode::Drag => Some(1002),
        MouseMode::Motion => Some(1003),
    };
    let mut out = String::new();
    for number in [1000, 1002, 1003] {
        out.push_str(&mode(number, level == Some(number)));
    }
    out.push_str(&mode(1006, level.is_some()));
    out
}
