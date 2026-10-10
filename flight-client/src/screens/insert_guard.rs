// SPDX-License-Identifier: MIT

/// The longest run of parameter bytes kept while waiting for a sequence's final byte. A sequence
/// with more is not one a program writes on purpose (the parser takes 32 parameters at most), and
/// is dropped.
const LONGEST: usize = 512;
const ESCAPE: u8 = 0x1b;
const CANCEL: u8 = 0x18;
const SUBSTITUTE: u8 = 0x1a;

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    /// After `ESC [`: the bytes of the sequence so far, and how long it is allowed to get.
    Csi(Vec<u8>),
    /// A sequence that went past [`LONGEST`], dropped up to its final byte.
    Dropping,
}

/// Keeps one escape sequence from costing the screen seconds: `CSI n @` (insert `n` blank cells)
/// makes the parser build a row `n` cells long one insertion at a time, which takes time that
/// grows with the square of `n` (a second for `n` = 65535, on a screen of any size). A row only
/// has `cols` cells, so inserting more than that is inserting exactly that many: the count is
/// written as `cols` and the screen comes out the same. Every other byte goes through as it came.
///
/// A sequence may arrive in pieces; its parameter bytes are held until its final byte, which
/// is when it can be told apart. (Nothing can be seen of a sequence before then.)
#[derive(Default)]
pub(super) struct InsertGuard {
    state: State,
}

impl InsertGuard {
    pub(super) fn pass(&mut self, bytes: &[u8], cols: u16) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            self.step(byte, cols, &mut out);
        }
        out
    }

    fn step(&mut self, byte: u8, cols: u16, out: &mut Vec<u8>) {
        match std::mem::take(&mut self.state) {
            State::Ground => {
                out.push(byte);
                if byte == ESCAPE {
                    self.state = State::Escape;
                }
            }
            State::Escape => {
                out.push(byte);
                self.state = match byte {
                    b'[' => State::Csi(Vec::new()),
                    ESCAPE => State::Escape,
                    _ => State::Ground,
                };
            }
            State::Csi(mut held) => match byte {
                // Cut short: what was held was never a sequence the parser acts on.
                ESCAPE => {
                    out.extend_from_slice(&held);
                    out.push(byte);
                    self.state = State::Escape;
                }
                CANCEL | SUBSTITUTE => {
                    out.extend_from_slice(&held);
                    out.push(byte);
                }
                0x40..=0x7e => finish(&held, byte, cols, out),
                0x00..=0x3f => {
                    if held.len() >= LONGEST {
                        self.state = State::Dropping;
                    } else {
                        held.push(byte);
                        self.state = State::Csi(held);
                    }
                }
                // Not part of a sequence at all.
                _ => {
                    out.extend_from_slice(&held);
                    out.push(byte);
                }
            },
            State::Dropping => match byte {
                ESCAPE => {
                    out.push(byte);
                    self.state = State::Escape;
                }
                CANCEL | SUBSTITUTE => out.push(byte),
                // The end of the sequence: a final byte the parser does nothing for, so the
                // `ESC [` already written is finished.
                0x40..=0x7e => out.push(b'~'),
                _ => self.state = State::Dropping,
            },
        }
    }
}

/// A sequence `ESC [ held final` is complete.
fn finish(held: &[u8], final_byte: u8, cols: u16, out: &mut Vec<u8>) {
    let plain = held
        .iter()
        .all(|b| b.is_ascii_digit() || matches!(b, b';' | b':') || *b < 0x20);
    if final_byte != b'@' || !plain {
        out.extend_from_slice(held);
        out.push(final_byte);
        return;
    }
    // The first parameter is the count; controls inside a sequence do not end it, and are
    // acted on whichever side of the digits they are, so they are kept in front.
    let end = held
        .iter()
        .position(|b| matches!(b, b';' | b':'))
        .unwrap_or(held.len());
    let (first, rest) = held.split_at(end);
    let mut count: u32 = 0;
    for digit in first.iter().filter(|b| b.is_ascii_digit()) {
        count = count
            .saturating_mul(10)
            .saturating_add(u32::from(digit.saturating_sub(b'0')));
    }
    if count <= u32::from(cols) {
        out.extend_from_slice(held);
    } else {
        out.extend(first.iter().filter(|b| **b < 0x20));
        out.extend_from_slice(cols.to_string().as_bytes());
        out.extend_from_slice(rest);
    }
    out.push(final_byte);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(guard: &mut InsertGuard, bytes: &[u8]) -> Vec<u8> {
        guard.pass(bytes, 80)
    }

    #[test]
    fn a_count_past_the_width_is_written_as_the_width_and_nothing_else_changes() {
        let mut g = InsertGuard::default();
        assert_eq!(pass(&mut g, b"a\x1b[65535@b"), b"a\x1b[80@b");
        assert_eq!(pass(&mut g, b"\x1b[99999999999;5@"), b"\x1b[80;5@");
        assert_eq!(pass(&mut g, b"\x1b[0000081:2@"), b"\x1b[80:2@");
        assert_eq!(pass(&mut g, b"\x1b[6\n5535@"), b"\x1b[\n80@");
        for same in [
            &b"\x1b[@"[..],
            b"\x1b[80@",
            b"\x1b[5@",
            b"\x1b[65535P",
            b"\x1b[65535 @",
            b"\x1b[?65535@",
            b"\x1b[38;5;200m",
            b"plain \xe6\x97\xa5 text \x1b[H",
        ] {
            assert_eq!(pass(&mut g, same), same);
        }
    }

    #[test]
    fn a_sequence_cut_between_writes_is_finished_by_the_next() {
        let mut g = InsertGuard::default();
        assert_eq!(pass(&mut g, b"x\x1b[65"), b"x\x1b[");
        assert_eq!(pass(&mut g, b"535"), b"");
        assert_eq!(pass(&mut g, b"@y"), b"80@y");
    }

    #[test]
    fn a_sequence_that_is_cut_short_goes_through_whole() {
        let mut g = InsertGuard::default();
        assert_eq!(pass(&mut g, b"\x1b[65535\x1b[3A"), b"\x1b[65535\x1b[3A");
        assert_eq!(pass(&mut g, b"\x1b[65535\x18@"), b"\x1b[65535\x18@");
    }

    #[test]
    fn a_sequence_of_absurd_length_is_dropped_to_its_end() {
        let mut g = InsertGuard::default();
        let long = [b"\x1b[".as_slice(), &[b'0'; 600], b"65535@z"].concat();
        assert_eq!(pass(&mut g, &long), b"\x1b[~z");
    }
}
