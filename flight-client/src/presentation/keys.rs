// SPDX-License-Identifier: MIT

use super::command::{Command, Shown};
use flight_present::{Axis, Direction};

/// The local escape byte, `Ctrl-Space`.
const ESCAPE: u8 = 0x00;

/// What the keyboard meant, in the order it was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Bytes for the focused surface.
    Data(Vec<u8>),
    Command(Command),
    Leave,
    /// `Ctrl-Space` and a key that means nothing: remind the user of the keys.
    Hint,
}

/// Splits the keyboard into bytes for the focused surface and commands for the presentation.
/// The prefix may arrive in a different read from its key. `Ctrl-Space` twice sends one literal
/// `Ctrl-Space`. Anything unknown is discarded with the prefix, so nothing is forwarded by
/// accident.
#[derive(Debug, Default)]
pub struct KeyFilter {
    prefix_seen: bool,
}

impl KeyFilter {
    pub fn keys(&mut self, input: &[u8]) -> Vec<Key> {
        let mut keys = Vec::new();
        let mut run = Vec::new();
        for &byte in input {
            if !self.prefix_seen {
                if byte == ESCAPE {
                    self.prefix_seen = true;
                } else {
                    run.push(byte);
                }
                continue;
            }
            self.prefix_seen = false;
            match byte {
                ESCAPE => run.push(ESCAPE),
                b'q' => {
                    flush(&mut run, &mut keys);
                    keys.push(Key::Leave);
                    return keys;
                }
                other => {
                    flush(&mut run, &mut keys);
                    keys.push(command(other).map_or(Key::Hint, Key::Command));
                }
            }
        }
        flush(&mut run, &mut keys);
        keys
    }
}

fn flush(run: &mut Vec<u8>, keys: &mut Vec<Key>) {
    if !run.is_empty() {
        keys.push(Key::Data(std::mem::take(run)));
    }
}

fn command(key: u8) -> Option<Command> {
    Some(match key {
        b'|' | b'%' => Command::Split(Axis::Across),
        b'-' | b'"' => Command::Split(Axis::Down),
        b't' => Command::NewTab,
        b'x' => Command::Close,
        b'n' => Command::StepTab(true),
        b'p' => Command::StepTab(false),
        b'h' => Command::Focus(Direction::Left),
        b'l' => Command::Focus(Direction::Right),
        b'k' => Command::Focus(Direction::Up),
        b'j' => Command::Focus(Direction::Down),
        b'o' => Command::FocusNext,
        b'>' => Command::Grow(Axis::Across, true),
        b'<' => Command::Grow(Axis::Across, false),
        b'+' => Command::Grow(Axis::Down, true),
        b'_' => Command::Grow(Axis::Down, false),
        b'a' => Command::ShowHere(Shown::Agent),
        b's' => Command::ShowHere(Shown::Shell),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_commands_and_the_literal_prefix_keep_their_order() {
        let mut f = KeyFilter::default();
        assert_eq!(
            f.keys(b"ab\x00|cd\x00\x00e\x00z"),
            vec![
                Key::Data(b"ab".to_vec()),
                Key::Command(Command::Split(Axis::Across)),
                Key::Data(b"cd\x00e".to_vec()),
                Key::Hint,
            ]
        );
    }

    #[test]
    fn a_prefix_and_its_key_may_arrive_apart_and_leaving_drops_the_rest() {
        let mut f = KeyFilter::default();
        assert_eq!(f.keys(b"x\x00"), vec![Key::Data(b"x".to_vec())]);
        assert_eq!(f.keys(b"q more"), vec![Key::Leave]);
    }
}
