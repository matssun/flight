// SPDX-License-Identifier: MIT

use crate::screens::{MouseEncoding, MouseMode};

/// One mouse report from the user's terminal (always the SGR form, which Flight switches on),
/// with the position counted from zero on the whole terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// The SGR button field: bits 0-1 the button, 4/8/16 shift, alt, ctrl, 32 motion, 64 wheel.
    pub code: u16,
    pub col: u16,
    pub row: u16,
    pub release: bool,
}

/// What an event is, as far as who is to be told of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    /// Movement with a button held.
    Drag,
    /// Movement with none.
    Move,
    Wheel,
}

const MOTION: u16 = 32;
const WHEEL: u16 = 64;
const NO_BUTTON: u16 = 3;

impl MouseEvent {
    pub fn kind(&self) -> Kind {
        if self.code & WHEEL != 0 {
            Kind::Wheel
        } else if self.code & MOTION != 0 {
            if self.code & NO_BUTTON == NO_BUTTON {
                Kind::Move
            } else {
                Kind::Drag
            }
        } else if self.release {
            Kind::Release
        } else {
            Kind::Press
        }
    }

    /// Whether a program in `mode` asked to be told of this.
    pub fn wanted_by(&self, mode: MouseMode) -> bool {
        match (mode, self.kind()) {
            (MouseMode::None, _) => false,
            (MouseMode::Press, Kind::Press | Kind::Wheel) => true,
            (MouseMode::Press, _) => false,
            (MouseMode::PressRelease, Kind::Press | Kind::Release | Kind::Wheel) => true,
            (MouseMode::PressRelease, _) => false,
            (MouseMode::Drag, Kind::Move) => false,
            (MouseMode::Drag | MouseMode::Motion, _) => true,
        }
    }

    /// The report for a program that `encoding` says how to write it to, with the position
    /// (from zero) in the program's own screen. `None` where the encoding cannot say it.
    pub fn report(&self, col: u16, row: u16, encoding: MouseEncoding) -> Option<Vec<u8>> {
        match encoding {
            MouseEncoding::Sgr => Some(
                format!(
                    "\x1b[<{};{};{}{}",
                    self.code,
                    u32::from(col).saturating_add(1),
                    u32::from(row).saturating_add(1),
                    if self.release { 'm' } else { 'M' }
                )
                .into_bytes(),
            ),
            MouseEncoding::Default | MouseEncoding::Utf8 => {
                // These cannot say which button was released: it is button 3.
                let code = if self.release {
                    self.code | NO_BUTTON
                } else {
                    self.code
                };
                let mut out = b"\x1b[M".to_vec();
                for value in [
                    u32::from(code),
                    u32::from(col).saturating_add(1),
                    u32::from(row).saturating_add(1),
                ] {
                    push_legacy(&mut out, value.checked_add(32)?, encoding)?;
                }
                Some(out)
            }
        }
    }
}

fn push_legacy(out: &mut Vec<u8>, value: u32, encoding: MouseEncoding) -> Option<()> {
    if encoding == MouseEncoding::Utf8 {
        let mut buf = [0u8; 4];
        out.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut buf).as_bytes());
    } else {
        out.push(u8::try_from(value).ok()?);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: u16) -> MouseEvent {
        MouseEvent {
            code,
            col: 0,
            row: 0,
            release: false,
        }
    }

    #[test]
    fn what_a_mode_asked_for_is_what_it_is_told() {
        let drag = press(32);
        let moved = press(35);
        let wheel = press(64);
        let release = MouseEvent {
            release: true,
            ..press(0)
        };
        assert!(press(0).wanted_by(MouseMode::Press) && !release.wanted_by(MouseMode::Press));
        assert!(release.wanted_by(MouseMode::PressRelease) && wheel.wanted_by(MouseMode::Press));
        assert!(!drag.wanted_by(MouseMode::PressRelease) && drag.wanted_by(MouseMode::Drag));
        assert!(!moved.wanted_by(MouseMode::Drag) && moved.wanted_by(MouseMode::Motion));
        assert!(!press(0).wanted_by(MouseMode::None));
    }

    #[test]
    fn each_encoding_writes_the_same_event_its_own_way() {
        let e = MouseEvent {
            code: 0,
            col: 4,
            row: 9,
            release: false,
        };
        assert_eq!(e.report(1, 2, MouseEncoding::Sgr).unwrap(), b"\x1b[<0;2;3M");
        let up = MouseEvent { release: true, ..e };
        assert_eq!(
            up.report(1, 2, MouseEncoding::Sgr).unwrap(),
            b"\x1b[<0;2;3m"
        );
        assert_eq!(
            up.report(1, 2, MouseEncoding::Default).unwrap(),
            b"\x1b[M#\"#"
        );
    }

    #[test]
    fn a_position_an_encoding_cannot_write_is_not_written() {
        let e = press(0);
        assert!(e.report(300, 0, MouseEncoding::Default).is_none());
        let mut utf8 = b"\x1b[M ".to_vec();
        utf8.extend_from_slice("\u{14d}!".as_bytes());
        assert_eq!(e.report(300, 0, MouseEncoding::Utf8).unwrap(), utf8);
        assert!(e.report(u16::MAX, 0, MouseEncoding::Sgr).is_some());
    }
}
