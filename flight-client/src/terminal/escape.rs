// SPDX-License-Identifier: MIT

/// The local escape byte, `Ctrl-]`. Handled before anything is forwarded, so a wedged remote
/// can always be left, and a literal `Ctrl-]` can still be sent.
const ESCAPE: u8 = 0x1d;

/// What the escape filter wants done besides forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeAction {
    None,
    /// `Ctrl-]` then `q`: leave the terminal.
    Leave,
    /// `Ctrl-]` then something else: nothing was forwarded; remind the user of the keys.
    Hint,
}

/// `Ctrl-]` then `q` leaves; `Ctrl-]` then `Ctrl-]` sends one literal `Ctrl-]`; `Ctrl-]`
/// followed by anything else is discarded together with the prefix (and a hint is due), so
/// nothing is forwarded by accident. The prefix may arrive in a different read from its key.
#[derive(Debug, Default)]
pub struct EscapeFilter {
    prefix_seen: bool,
}

impl EscapeFilter {
    /// The bytes to forward to the remote, and what else to do. After `Leave`, the rest of
    /// the input is dropped.
    pub fn feed(&mut self, input: &[u8]) -> (Vec<u8>, EscapeAction) {
        let mut out = Vec::with_capacity(input.len());
        let mut action = EscapeAction::None;
        for &b in input {
            if self.prefix_seen {
                self.prefix_seen = false;
                match b {
                    b'q' | b'Q' => return (out, EscapeAction::Leave),
                    ESCAPE => out.push(ESCAPE),
                    _ => action = EscapeAction::Hint,
                }
            } else if b == ESCAPE {
                self.prefix_seen = true;
            } else {
                out.push(b);
            }
        }
        (out, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_input_passes_through_untouched() {
        let mut f = EscapeFilter::default();
        assert_eq!(
            f.feed(b"ls -l\r"),
            (b"ls -l\r".to_vec(), EscapeAction::None)
        );
    }

    #[test]
    fn ctrl_bracket_q_leaves_and_drops_what_follows() {
        let mut f = EscapeFilter::default();
        assert_eq!(f.feed(b"ab\x1dqcd"), (b"ab".to_vec(), EscapeAction::Leave));
    }

    #[test]
    fn a_doubled_prefix_sends_one_literal_ctrl_bracket() {
        let mut f = EscapeFilter::default();
        assert_eq!(
            f.feed(b"a\x1d\x1db"),
            (b"a\x1db".to_vec(), EscapeAction::None)
        );
    }

    #[test]
    fn any_other_key_after_the_prefix_forwards_nothing_and_asks_for_a_hint() {
        let mut f = EscapeFilter::default();
        assert_eq!(f.feed(b"a\x1dxb"), (b"ab".to_vec(), EscapeAction::Hint));
    }

    #[test]
    fn the_prefix_and_its_key_may_arrive_separately() {
        let mut f = EscapeFilter::default();
        assert_eq!(f.feed(b"\x1d"), (Vec::new(), EscapeAction::None));
        assert_eq!(f.feed(b"q"), (Vec::new(), EscapeAction::Leave));
        let mut f = EscapeFilter::default();
        f.feed(b"\x1d");
        assert_eq!(f.feed(b"\x1d"), (vec![0x1d], EscapeAction::None));
    }

    #[test]
    fn the_prefix_state_does_not_leak_after_a_completed_sequence() {
        let mut f = EscapeFilter::default();
        f.feed(b"\x1d\x1d");
        assert_eq!(f.feed(b"q"), (b"q".to_vec(), EscapeAction::None));
    }
}
