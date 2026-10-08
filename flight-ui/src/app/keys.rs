// SPDX-License-Identifier: MIT

use crate::view::{Action, FilterInput, FormInput, InputMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Terminal keys to actions, by where keys are going: while the new-session form is open or a
/// search is being typed, letters type instead of acting.
pub fn action_for(key: KeyEvent, mode: InputMode) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c')).then_some(Action::Quit);
    }
    match mode {
        InputMode::Form => form_action(key).map(Action::Form),
        InputMode::Help => Some(Action::CloseHelp),
        InputMode::Search => search_action(key),
        InputMode::Dashboard => dashboard_action(key),
    }
}

fn dashboard_action(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
        KeyCode::Enter => Some(Action::Switch),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('n') => Some(Action::NewSession),
        KeyCode::Char('/') => Some(Action::Search),
        KeyCode::Char('?') => Some(Action::Help),
        KeyCode::Char('q') => Some(Action::Quit),
        // Esc leaves a search first; only with none does it quit.
        KeyCode::Esc => Some(Action::Back),
        _ => None,
    }
}

fn search_action(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::Filter(FilterInput::Clear)),
        KeyCode::Enter => Some(Action::Filter(FilterInput::Accept)),
        KeyCode::Backspace => Some(Action::Filter(FilterInput::Backspace)),
        KeyCode::Up => Some(Action::Up),
        KeyCode::Down => Some(Action::Down),
        KeyCode::Char(c) => Some(Action::Filter(FilterInput::Char(c))),
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

    fn dash(code: KeyCode) -> Option<Action> {
        action_for(k(code), InputMode::Dashboard)
    }

    #[test]
    fn the_dashboard_key_set() {
        assert_eq!(dash(KeyCode::Up), Some(Action::Up));
        assert_eq!(dash(KeyCode::Char('j')), Some(Action::Down));
        assert_eq!(dash(KeyCode::Enter), Some(Action::Switch));
        assert_eq!(dash(KeyCode::Char('r')), Some(Action::Refresh));
        assert_eq!(dash(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(dash(KeyCode::Char('n')), Some(Action::NewSession));
        assert_eq!(dash(KeyCode::Char('/')), Some(Action::Search));
        assert_eq!(dash(KeyCode::Char('?')), Some(Action::Help));
        assert_eq!(dash(KeyCode::Esc), Some(Action::Back));
        assert_eq!(dash(KeyCode::Char('x')), None);
    }

    #[test]
    fn ctrl_c_quits_and_other_ctrl_chords_do_nothing() {
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        for mode in [InputMode::Dashboard, InputMode::Form, InputMode::Search] {
            assert_eq!(action_for(ctrl('c'), mode), Some(Action::Quit));
            assert_eq!(action_for(ctrl('r'), mode), None);
        }
    }

    #[test]
    fn in_the_form_keys_type_instead_of_acting() {
        let typed = |c| action_for(k(KeyCode::Char(c)), InputMode::Form);
        // `q` and `n` are letters in a name, not commands.
        assert_eq!(typed('q'), Some(Action::Form(FormInput::Char('q'))));
        assert_eq!(typed('n'), Some(Action::Form(FormInput::Char('n'))));
        assert_eq!(
            action_for(k(KeyCode::Esc), InputMode::Form),
            Some(Action::Form(FormInput::Cancel))
        );
        assert_eq!(
            action_for(
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                InputMode::Form
            ),
            Some(Action::Form(FormInput::Prev))
        );
    }

    #[test]
    fn while_searching_letters_type_and_escape_clears() {
        let s = |code| action_for(k(code), InputMode::Search);
        assert_eq!(
            s(KeyCode::Char('q')),
            Some(Action::Filter(FilterInput::Char('q')))
        );
        assert_eq!(s(KeyCode::Esc), Some(Action::Filter(FilterInput::Clear)));
        assert_eq!(s(KeyCode::Enter), Some(Action::Filter(FilterInput::Accept)));
        assert_eq!(s(KeyCode::Down), Some(Action::Down));
    }

    #[test]
    fn any_key_closes_help() {
        assert_eq!(
            action_for(k(KeyCode::Char('x')), InputMode::Help),
            Some(Action::CloseHelp)
        );
    }
}
