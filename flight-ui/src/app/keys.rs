// SPDX-License-Identifier: MIT

use crate::view::{Action, FormInput};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Terminal keys to actions. While the new-session form is open, keys type into it instead.
pub fn action_for(key: KeyEvent, form_open: bool) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c')).then_some(Action::Quit);
    }
    if form_open {
        return form_action(key).map(Action::Form);
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
        KeyCode::Enter => Some(Action::Switch),
        KeyCode::Tab => Some(Action::ToggleFocus),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('n') => Some(Action::NewSession),
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        _ => None,
    }
}

fn form_action(key: KeyEvent) -> Option<FormInput> {
    match key.code {
        KeyCode::Esc => Some(FormInput::Cancel),
        KeyCode::Enter => Some(FormInput::Enter),
        KeyCode::Tab | KeyCode::Down => Some(FormInput::Next),
        KeyCode::BackTab | KeyCode::Up => Some(FormInput::Prev),
        KeyCode::Left => Some(FormInput::Left),
        KeyCode::Right => Some(FormInput::Right),
        KeyCode::Backspace => Some(FormInput::Backspace),
        KeyCode::Char(c) => Some(FormInput::Char(c)),
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
        assert_eq!(action_for(k(KeyCode::Up), false), Some(Action::Up));
        assert_eq!(action_for(k(KeyCode::Down), false), Some(Action::Down));
        assert_eq!(action_for(k(KeyCode::Enter), false), Some(Action::Switch));
        assert_eq!(
            action_for(k(KeyCode::Tab), false),
            Some(Action::ToggleFocus)
        );
        assert_eq!(
            action_for(k(KeyCode::Char('r')), false),
            Some(Action::Refresh)
        );
        assert_eq!(action_for(k(KeyCode::Char('q')), false), Some(Action::Quit));
        assert_eq!(
            action_for(k(KeyCode::Char('n')), false),
            Some(Action::NewSession)
        );
        assert_eq!(action_for(k(KeyCode::Char('x')), false), None);
    }

    #[test]
    fn ctrl_c_quits_and_other_ctrl_chords_do_nothing() {
        assert_eq!(
            action_for(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for(
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
                false
            ),
            None
        );
    }

    #[test]
    fn in_the_form_keys_type_instead_of_acting() {
        let typed = |c| action_for(k(KeyCode::Char(c)), true);
        // `q` and `n` are letters in a name, not commands.
        assert_eq!(typed('q'), Some(Action::Form(FormInput::Char('q'))));
        assert_eq!(typed('n'), Some(Action::Form(FormInput::Char('n'))));
        assert_eq!(
            action_for(k(KeyCode::Esc), true),
            Some(Action::Form(FormInput::Cancel))
        );
        assert_eq!(
            action_for(k(KeyCode::Tab), true),
            Some(Action::Form(FormInput::Next))
        );
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), true),
            Some(Action::Form(FormInput::Prev))
        );
        // Ctrl-C still leaves the program.
        assert_eq!(
            action_for(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                true
            ),
            Some(Action::Quit)
        );
    }
}
