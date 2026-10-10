// SPDX-License-Identifier: MIT

use crate::session::InputEvent;
use flight_ui::SurfaceChoice;

/// The local escape byte, `Ctrl-Space`. Handled before anything is forwarded, so a wedged remote
/// can always be left, and a literal `Ctrl-Space` can still be sent.
const ESCAPE: u8 = 0x00;

/// `Ctrl-Space` then `q` leaves; `v` asks for the surfaces side by side; then `a` or `s` switches
/// to the workspace's agent or shell;
/// then `Ctrl-Space` again sends one literal `Ctrl-Space`; followed by anything else it is
/// discarded together with the prefix (and a hint is due), so nothing is forwarded by
/// accident. The prefix may arrive in a different read from its key.
#[derive(Debug, Default)]
pub struct EscapeFilter {
    prefix_seen: bool,
    /// The surface being shown, if known: asking for it again does nothing.
    showing: Option<SurfaceChoice>,
    /// What was typed after `Ctrl-Space v` in the same read: it is for what shows the surfaces
    /// side by side, not for this.
    unread: Vec<u8>,
}

impl EscapeFilter {
    /// A filter for a terminal that shows `showing`.
    pub fn showing(showing: Option<SurfaceChoice>) -> Self {
        Self {
            prefix_seen: false,
            showing,
            unread: Vec::new(),
        }
    }

    /// What came after `Ctrl-Space v` in the read that held it, once.
    pub fn take_unread(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.unread)
    }

    /// The surface now on screen, after a switch that did not happen.
    pub fn set_showing(&mut self, showing: Option<SurfaceChoice>) {
        self.showing = showing;
    }

    /// Everything the input means, in the order it was typed. Nothing after a
    /// switch is dropped: the bytes that follow `Ctrl-Space s` are for the
    /// shell, and are returned as data after the switch event. After `Leave` the rest is
    /// dropped (the session is over).
    pub fn events(&mut self, input: &[u8]) -> Vec<InputEvent> {
        let mut events = Vec::new();
        let mut run = Vec::new();
        let flush = |run: &mut Vec<u8>, events: &mut Vec<InputEvent>| {
            if !run.is_empty() {
                events.push(InputEvent::Data(std::mem::take(run)));
            }
        };
        for (at, &b) in input.iter().enumerate() {
            if self.prefix_seen {
                self.prefix_seen = false;
                match b {
                    b'q' | b'Q' => {
                        flush(&mut run, &mut events);
                        events.push(InputEvent::Leave);
                        return events;
                    }
                    b'v' | b'V' => {
                        flush(&mut run, &mut events);
                        events.push(InputEvent::Present);
                        self.unread = input
                            .get(at.saturating_add(1)..)
                            .unwrap_or_default()
                            .to_vec();
                        return events;
                    }
                    b'a' | b'A' | b's' | b'S' => {
                        let wanted = if matches!(b, b'a' | b'A') {
                            SurfaceChoice::Agent
                        } else {
                            SurfaceChoice::Shell
                        };
                        if self.showing != Some(wanted) {
                            flush(&mut run, &mut events);
                            self.showing = Some(wanted);
                            events.push(InputEvent::Switch(wanted));
                        }
                    }
                    ESCAPE => run.push(ESCAPE),
                    _ => {
                        flush(&mut run, &mut events);
                        events.push(InputEvent::Hint);
                    }
                }
            } else if b == ESCAPE {
                self.prefix_seen = true;
            } else {
                run.push(b);
            }
        }
        flush(&mut run, &mut events);
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_what_is_typed_after_a_switch_for_the_new_surface() {
        let mut f = EscapeFilter::showing(Some(SurfaceChoice::Agent));
        assert_eq!(
            f.events(b"ab\x00sls\r\x00aok"),
            vec![
                InputEvent::Data(b"ab".to_vec()),
                InputEvent::Switch(SurfaceChoice::Shell),
                InputEvent::Data(b"ls\r".to_vec()),
                InputEvent::Switch(SurfaceChoice::Agent),
                InputEvent::Data(b"ok".to_vec()),
            ]
        );
    }

    #[test]
    fn events_do_not_switch_to_the_surface_already_wanted() {
        let mut f = EscapeFilter::showing(Some(SurfaceChoice::Agent));
        assert_eq!(
            f.events(b"\x00s\x00sx"),
            vec![
                InputEvent::Switch(SurfaceChoice::Shell),
                InputEvent::Data(b"x".to_vec())
            ]
        );
    }

    #[test]
    fn what_follows_a_request_for_both_in_the_same_read_is_kept_for_what_comes_next() {
        let mut f = EscapeFilter::default();
        assert_eq!(
            f.events(b"a\x00vtyped-after"),
            vec![InputEvent::Data(b"a".to_vec()), InputEvent::Present]
        );
        assert_eq!(f.take_unread(), b"typed-after");
        assert_eq!(f.take_unread(), b"");
    }

    #[test]
    fn events_stop_at_leave_hint_and_literal_prefix_work_across_reads() {
        let mut f = EscapeFilter::default();
        assert_eq!(f.events(b"a\x00"), vec![InputEvent::Data(b"a".to_vec())]);
        assert_eq!(
            f.events(b"\x00b"),
            vec![InputEvent::Data(b"\x00b".to_vec())]
        );
        assert_eq!(
            f.events(b"\x00xc"),
            vec![InputEvent::Hint, InputEvent::Data(b"c".to_vec())]
        );
        assert_eq!(
            f.events(b"d\x00qe"),
            vec![InputEvent::Data(b"d".to_vec()), InputEvent::Leave]
        );
    }
}
