// SPDX-License-Identifier: MIT

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The bytes a terminal would have sent for this key, for keys typed after the user opened a
/// surface and before the dashboard let go of the keyboard. They belong to the surface, not to
/// the dashboard, so they are handed on instead of being read as dashboard commands. `None` for
/// keys with no byte form here; the caller counts those instead of guessing.
pub(super) fn encode_key(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let mut bytes = match key.code {
        KeyCode::Char(c) if ctrl => vec![control_byte(c)?],
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => cursor(b'A', ctrl, alt, shift),
        KeyCode::Down => cursor(b'B', ctrl, alt, shift),
        KeyCode::Right => cursor(b'C', ctrl, alt, shift),
        KeyCode::Left => cursor(b'D', ctrl, alt, shift),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(n) => function_key(n)?,
        _ => return None,
    };
    // Alt is an escape before the key, except where it is already part of a sequence.
    if alt
        && matches!(
            key.code,
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Enter
        )
    {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn control_byte(c: char) -> Option<u8> {
    match c {
        ' ' | '@' | '2' => Some(0x00),
        'a'..='z' => u8::try_from(u32::from(c).checked_sub(u32::from('a'))?.checked_add(1)?).ok(),
        'A'..='Z' => u8::try_from(u32::from(c).checked_sub(u32::from('A'))?.checked_add(1)?).ok(),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}

fn cursor(final_byte: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    if modifier == 1 {
        vec![0x1b, b'[', final_byte]
    } else {
        format!("\x1b[1;{modifier}{}", char::from(final_byte)).into_bytes()
    }
}

fn function_key(n: u8) -> Option<Vec<u8>> {
    let sequence: &[u8] = match n {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(sequence.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> Option<Vec<u8>> {
        encode_key(&key(code, KeyModifiers::NONE))
    }

    #[test]
    fn text_and_the_common_editing_keys_are_what_a_terminal_sends() {
        assert_eq!(plain(KeyCode::Char('a')), Some(b"a".to_vec()));
        assert_eq!(plain(KeyCode::Char('é')), Some("é".as_bytes().to_vec()));
        assert_eq!(plain(KeyCode::Enter), Some(b"\r".to_vec()));
        assert_eq!(plain(KeyCode::Tab), Some(b"\t".to_vec()));
        assert_eq!(plain(KeyCode::Backspace), Some(vec![0x7f]));
        assert_eq!(plain(KeyCode::Esc), Some(vec![0x1b]));
        assert_eq!(plain(KeyCode::Delete), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn control_keys_are_control_bytes_and_ctrl_space_is_nul() {
        let ctrl = |c| encode_key(&key(KeyCode::Char(c), KeyModifiers::CONTROL));
        assert_eq!(ctrl('c'), Some(vec![3]));
        assert_eq!(ctrl('A'), Some(vec![1]));
        assert_eq!(ctrl('z'), Some(vec![26]));
        assert_eq!(ctrl(' '), Some(vec![0]));
        assert_eq!(ctrl('['), Some(vec![0x1b]));
        assert_eq!(ctrl('9'), None);
    }

    #[test]
    fn alt_prefixes_an_escape() {
        assert_eq!(
            encode_key(&key(KeyCode::Char('f'), KeyModifiers::ALT)),
            Some(b"\x1bf".to_vec())
        );
    }

    #[test]
    fn arrows_carry_their_modifiers() {
        assert_eq!(plain(KeyCode::Up), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            encode_key(&key(KeyCode::Right, KeyModifiers::CONTROL)),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            encode_key(&key(KeyCode::Left, KeyModifiers::SHIFT | KeyModifiers::ALT)),
            Some(b"\x1b[1;4D".to_vec())
        );
    }

    #[test]
    fn function_keys_and_unknown_keys() {
        assert_eq!(plain(KeyCode::F(1)), Some(b"\x1bOP".to_vec()));
        assert_eq!(plain(KeyCode::F(5)), Some(b"\x1b[15~".to_vec()));
        assert_eq!(plain(KeyCode::F(30)), None);
        assert_eq!(plain(KeyCode::CapsLock), None);
    }
}
