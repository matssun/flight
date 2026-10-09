// SPDX-License-Identifier: MIT

use crate::session::InputEvent;
use std::collections::VecDeque;

/// The user's input waiting for its attachment, in order and bounded.
///
/// The bound is in bytes of data. When it is reached the session stops reading the keyboard, so
/// the terminal's own input buffer holds what is typed next and nothing is dropped. A single
/// read can overshoot by at most its own length. Events that carry no bytes (switches, hints)
/// are merged with a neighbour of the same kind and weigh one byte each, so alternating hints
/// and switches cannot grow the queue without bound either.
#[derive(Debug)]
pub struct InputQueue {
    events: VecDeque<InputEvent>,
    bytes: usize,
    limit: usize,
    left: bool,
}

impl InputQueue {
    pub fn new(limit: usize) -> Self {
        Self {
            events: VecDeque::new(),
            bytes: 0,
            limit,
            left: false,
        }
    }

    /// Whether more input may be read. False once the limit is reached or the user has left.
    pub fn has_room(&self) -> bool {
        !self.left && self.bytes < self.limit
    }

    pub fn queued_bytes(&self) -> usize {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn front(&self) -> Option<&InputEvent> {
        self.events.front()
    }

    /// Queue an event. Input after `Leave` is ignored: the session is over.
    pub fn push(&mut self, event: InputEvent) {
        if self.left {
            return;
        }
        match event {
            InputEvent::Data(bytes) if bytes.is_empty() => {}
            InputEvent::Data(bytes) => {
                self.bytes = self.bytes.saturating_add(bytes.len());
                if let Some(InputEvent::Data(last)) = self.events.back_mut() {
                    last.extend_from_slice(&bytes);
                } else {
                    self.events.push_back(InputEvent::Data(bytes));
                }
            }
            InputEvent::Switch(choice) => {
                // Two switches with nothing typed between them: only the last matters.
                if matches!(self.events.back(), Some(InputEvent::Switch(_))) {
                    self.events.pop_back();
                } else {
                    self.bytes = self.bytes.saturating_add(1);
                }
                self.events.push_back(InputEvent::Switch(choice));
            }
            InputEvent::Hint => {
                if !matches!(self.events.back(), Some(InputEvent::Hint)) {
                    self.bytes = self.bytes.saturating_add(1);
                    self.events.push_back(InputEvent::Hint);
                }
            }
            InputEvent::Leave => {
                self.left = true;
                self.events.push_back(InputEvent::Leave);
            }
        }
    }

    /// Take the next event, but no more than `max` bytes of data (the rest stays queued).
    pub fn pop(&mut self, max: usize) -> Option<InputEvent> {
        match self.events.front_mut()? {
            InputEvent::Data(bytes) if bytes.len() > max => {
                let rest = bytes.split_off(max);
                let head = std::mem::replace(bytes, rest);
                self.bytes = self.bytes.saturating_sub(head.len());
                Some(InputEvent::Data(head))
            }
            _ => {
                let event = self.events.pop_front()?;
                let weight = match &event {
                    InputEvent::Data(bytes) => bytes.len(),
                    InputEvent::Switch(_) | InputEvent::Hint => 1,
                    InputEvent::Leave => 0,
                };
                self.bytes = self.bytes.saturating_sub(weight);
                Some(event)
            }
        }
    }

    /// Drop the data that is next in line, up to the next event that is not data, and say how
    /// many bytes that was. Used when the surface the data was typed for cannot be reached: it
    /// is reported, never handed to a different surface.
    pub fn discard_data(&mut self) -> usize {
        let mut dropped = 0usize;
        if let Some(InputEvent::Data(_)) = self.events.front() {
            if let Some(InputEvent::Data(bytes)) = self.events.pop_front() {
                dropped = bytes.len();
                self.bytes = self.bytes.saturating_sub(dropped);
            }
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flight_ui::SurfaceChoice::{Agent, Shell};

    fn data(s: &str) -> InputEvent {
        InputEvent::Data(s.as_bytes().to_vec())
    }

    #[test]
    fn data_is_kept_in_order_and_merged() {
        let mut q = InputQueue::new(100);
        q.push(data("ab"));
        q.push(data("cd"));
        q.push(InputEvent::Switch(Shell));
        q.push(data("ef"));
        assert_eq!(q.queued_bytes(), 7);
        assert_eq!(q.pop(100), Some(data("abcd")));
        assert_eq!(q.pop(100), Some(InputEvent::Switch(Shell)));
        assert_eq!(q.pop(100), Some(data("ef")));
        assert_eq!(q.pop(100), None);
        assert_eq!(q.queued_bytes(), 0);
    }

    #[test]
    fn a_switch_after_a_switch_replaces_it_but_never_crosses_data() {
        let mut q = InputQueue::new(100);
        q.push(InputEvent::Switch(Shell));
        q.push(InputEvent::Switch(Agent));
        q.push(data("x"));
        q.push(InputEvent::Switch(Shell));
        assert_eq!(q.pop(100), Some(InputEvent::Switch(Agent)));
        assert_eq!(q.pop(100), Some(data("x")));
        assert_eq!(q.pop(100), Some(InputEvent::Switch(Shell)));
    }

    #[test]
    fn the_limit_stops_reading_and_a_pop_makes_room_again() {
        let mut q = InputQueue::new(4);
        assert!(q.has_room());
        q.push(data("abcd"));
        assert!(!q.has_room());
        assert_eq!(q.pop(2), Some(data("ab")));
        assert_eq!(q.queued_bytes(), 2);
        assert!(q.has_room());
    }

    #[test]
    fn a_long_read_overshoots_the_limit_by_at_most_itself() {
        let mut q = InputQueue::new(4);
        q.push(data("abc"));
        assert!(q.has_room());
        q.push(data("defghij"));
        assert_eq!(q.queued_bytes(), 10);
        assert!(!q.has_room());
    }

    #[test]
    fn repeated_hints_and_switches_cannot_grow_the_queue() {
        let mut q = InputQueue::new(4);
        let mut pushed = 0;
        while q.has_room() {
            q.push(InputEvent::Hint);
            q.push(InputEvent::Switch(Shell));
            pushed += 1;
            assert!(pushed < 100, "the queue never filled");
        }
        assert!(q.queued_bytes() <= 5);
        assert_eq!(q.pop(10), Some(InputEvent::Hint));
        assert_eq!(q.pop(10), Some(InputEvent::Switch(Shell)));
    }

    #[test]
    fn nothing_is_accepted_after_leave() {
        let mut q = InputQueue::new(100);
        q.push(data("a"));
        q.push(InputEvent::Leave);
        q.push(data("b"));
        assert!(!q.has_room());
        assert_eq!(q.pop(10), Some(data("a")));
        assert_eq!(q.pop(10), Some(InputEvent::Leave));
        assert_eq!(q.pop(10), None);
    }

    #[test]
    fn discarding_takes_only_the_data_in_front() {
        let mut q = InputQueue::new(100);
        q.push(data("abc"));
        q.push(InputEvent::Switch(Agent));
        q.push(data("de"));
        assert_eq!(q.discard_data(), 3);
        assert_eq!(q.discard_data(), 0);
        assert_eq!(q.front(), Some(&InputEvent::Switch(Agent)));
        assert_eq!(q.queued_bytes(), 3);
    }
}
