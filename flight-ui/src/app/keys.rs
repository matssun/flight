// SPDX-License-Identifier: MIT

use crate::view::Action;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Terminal keys to actions. The whole v0 key set.
pub fn action_for(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c')).then_some(Action::Quit);
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
        KeyCode::Enter => Some(Action::Switch),
        KeyCode::Tab => Some(Action::ToggleFocus),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_v0_key_set() {
        assert_eq!(action_for(k(KeyCode::Up)), Some(Action::Up));
        assert_eq!(action_for(k(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(k(KeyCode::Enter)), Some(Action::Switch));
        assert_eq!(action_for(k(KeyCode::Tab)), Some(Action::ToggleFocus));
        assert_eq!(action_for(k(KeyCode::Char('r'))), Some(Action::Refresh));
        assert_eq!(action_for(k(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(action_for(k(KeyCode::Char('x'))), None);
    }

    #[test]
    fn ctrl_c_quits_and_other_ctrl_chords_do_nothing() {
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            None
        );
    }
}
