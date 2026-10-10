// SPDX-License-Identifier: MIT

use super::mouse_event::MouseEvent;

/// The longest report there is: `ESC [ <` and three numbers of up to five digits.
const LONGEST: usize = 24;
const START: &[u8] = b"\x1b[<";

/// What the user's terminal sent, in order: keys, and mouse reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Bytes(Vec<u8>),
    Mouse(MouseEvent),
}

/// Takes the mouse reports out of the input (the SGR form, which is what Flight asks the
/// terminal for) and leaves every other byte as it was. A report cut between two reads is
/// held until its rest arrives; a lone `Esc`, or one that starts anything else, is never held,
/// so the key it is stays immediate.
#[derive(Debug, Default)]
pub struct MouseFilter {
    held: Vec<u8>,
}

impl MouseFilter {
    pub fn split(&mut self, input: &[u8]) -> Vec<Input> {
        let mut data = std::mem::take(&mut self.held);
        data.extend_from_slice(input);
        let mut out = Vec::new();
        let mut run = Vec::new();
        let mut at = 0;
        while let Some(&byte) = data.get(at) {
            let rest = data.get(at..).unwrap_or_default();
            if byte == 0x1b && rest.starts_with(START) {
                match report(rest) {
                    Parsed::Event(event, used) => {
                        flush(&mut run, &mut out);
                        out.push(Input::Mouse(event));
                        at = at.saturating_add(used);
                        continue;
                    }
                    Parsed::Incomplete => {
                        self.held = rest.to_vec();
                        break;
                    }
                    Parsed::Not => {}
                }
            }
            run.push(byte);
            at = at.saturating_add(1);
        }
        flush(&mut run, &mut out);
        out
    }
}

fn flush(run: &mut Vec<u8>, out: &mut Vec<Input>) {
    if !run.is_empty() {
        out.push(Input::Bytes(std::mem::take(run)));
    }
}

enum Parsed {
    Event(MouseEvent, usize),
    /// Could still become a report with more input.
    Incomplete,
    Not,
}

/// `rest` starts with `ESC [ <`.
fn report(rest: &[u8]) -> Parsed {
    let body = rest.get(START.len()..).unwrap_or_default();
    let Some(end) = body
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b';'))
    else {
        return if rest.len() < LONGEST {
            Parsed::Incomplete
        } else {
            Parsed::Not
        };
    };
    let release = match body.get(end) {
        Some(b'M') => false,
        Some(b'm') => true,
        _ => return Parsed::Not,
    };
    let numbers: Option<Vec<u16>> = body
        .get(..end)
        .and_then(|s| std::str::from_utf8(s).ok())
        .map(|s| s.split(';').map(|n| n.parse::<u16>().ok()).collect())
        .unwrap_or(None);
    match numbers.as_deref() {
        Some([code, col, row]) if *col >= 1 && *row >= 1 => Parsed::Event(
            MouseEvent {
                code: *code,
                col: col.saturating_sub(1),
                row: row.saturating_sub(1),
                release,
            },
            START.len().saturating_add(end).saturating_add(1),
        ),
        _ => Parsed::Not,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(code: u16, col: u16, row: u16, release: bool) -> Input {
        Input::Mouse(MouseEvent {
            code,
            col,
            row,
            release,
        })
    }

    #[test]
    fn reports_are_taken_out_and_the_keys_around_them_stay_in_order() {
        let mut f = MouseFilter::default();
        assert_eq!(
            f.split(b"ab\x1b[<0;10;5Mcd\x1b[<0;10;5me"),
            vec![
                Input::Bytes(b"ab".to_vec()),
                event(0, 9, 4, false),
                Input::Bytes(b"cd".to_vec()),
                event(0, 9, 4, true),
                Input::Bytes(b"e".to_vec()),
            ]
        );
    }

    #[test]
    fn a_report_cut_between_reads_is_held_and_finished() {
        let mut f = MouseFilter::default();
        assert_eq!(f.split(b"x\x1b[<64;1"), vec![Input::Bytes(b"x".to_vec())]);
        assert_eq!(
            f.split(b"2;3Mz"),
            vec![event(64, 11, 2, false), Input::Bytes(b"z".to_vec())]
        );
    }

    #[test]
    fn an_escape_that_is_not_a_report_is_never_held_or_changed() {
        let mut f = MouseFilter::default();
        for input in [
            &b"\x1b"[..],
            b"\x1b[",
            b"\x1b[A",
            b"\x1b[<x",
            b"\x1b[<0;0;0M",
            b"\x1b[<1;2M",
        ] {
            assert_eq!(
                f.split(input),
                vec![Input::Bytes(input.to_vec())],
                "{input:?}"
            );
        }
        let long = [b"\x1b[<".as_slice(), &[b'1'; 30]].concat();
        assert_eq!(f.split(&long), vec![Input::Bytes(long.clone())]);
    }
}
