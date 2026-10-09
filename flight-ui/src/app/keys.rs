// SPDX-License-Identifier: MIT

use crate::view::{
    Action, FilterInput, FormInput, InputMode, PromptInput, SavedOp, SavedPromptInput,
    SurfaceChoice,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Terminal keys to actions, by where keys are going: while the new-session form is open or a
/// search is being typed, letters type instead of acting.
pub fn action_for(key: KeyEvent, mode: InputMode) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c')).then_some(Action::Quit);
    }
    match mode {
        InputMode::Form => form_action(key).map(Action::Form),
        InputMode::Prompt => prompt_action(key).map(Action::Prompt),
        InputMode::SavedConfirm => saved_confirm_action(key).map(Action::SavedPrompt),
        InputMode::SavedInput => saved_input_action(key).map(Action::SavedPrompt),
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
        KeyCode::Char('a') => Some(Action::Open(SurfaceChoice::Agent)),
        KeyCode::Char('s') => Some(Action::Open(SurfaceChoice::Shell)),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('n') => Some(Action::NewSession),
        // These act only on a selected saved workspace.
        KeyCode::Char('c') => Some(Action::SavedOp(SavedOp::ChangeRoot)),
        KeyCode::Char('x') => Some(Action::SavedOp(SavedOp::Remove)),
        KeyCode::Char('v') => Some(Action::SavedOp(SavedOp::AcceptRoot)),
        KeyCode::Char('t') => Some(Action::SavedOp(SavedOp::Trust)),
        KeyCode::Char('f') => Some(Action::SavedOp(SavedOp::Fresh)),
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

fn prompt_action(key: KeyEvent) -> Option<PromptInput> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n' | 'N') => Some(PromptInput::Cancel),
        KeyCode::Char('y' | 'Y') => Some(PromptInput::Yes),
        KeyCode::Enter => Some(PromptInput::Enter),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => Some(PromptInput::Next),
        _ => None,
    }
}

fn saved_confirm_action(key: KeyEvent) -> Option<SavedPromptInput> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n' | 'N') => Some(SavedPromptInput::Cancel),
        KeyCode::Char('y' | 'Y') => Some(SavedPromptInput::Yes),
        KeyCode::Enter => Some(SavedPromptInput::Enter),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
            Some(SavedPromptInput::Next)
        }
        _ => None,
    }
}

fn saved_input_action(key: KeyEvent) -> Option<SavedPromptInput> {
    match key.code {
        KeyCode::Esc => Some(SavedPromptInput::Cancel),
        KeyCode::Enter => Some(SavedPromptInput::Enter),
        KeyCode::Tab | KeyCode::BackTab => Some(SavedPromptInput::Next),
        KeyCode::Backspace => Some(SavedPromptInput::Backspace),
        KeyCode::Char(c) => Some(SavedPromptInput::Char(c)),
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
        assert_eq!(
            dash(KeyCode::Char('a')),
            Some(Action::Open(SurfaceChoice::Agent))
        );
        assert_eq!(
            dash(KeyCode::Char('s')),
            Some(Action::Open(SurfaceChoice::Shell))
        );
        assert_eq!(
            dash(KeyCode::Char('x')),
            Some(Action::SavedOp(SavedOp::Remove))
        );
        assert_eq!(dash(KeyCode::Char('z')), None);
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
    fn the_shell_prompt_takes_yes_no_enter_and_escape() {
        let p = |code| action_for(k(code), InputMode::Prompt);
        assert_eq!(
            p(KeyCode::Char('y')),
            Some(Action::Prompt(PromptInput::Yes))
        );
        assert_eq!(
            p(KeyCode::Char('n')),
            Some(Action::Prompt(PromptInput::Cancel))
        );
        assert_eq!(p(KeyCode::Esc), Some(Action::Prompt(PromptInput::Cancel)));
        assert_eq!(p(KeyCode::Enter), Some(Action::Prompt(PromptInput::Enter)));
        assert_eq!(p(KeyCode::Tab), Some(Action::Prompt(PromptInput::Next)));
        // Letters that mean something on the dashboard do nothing here.
        assert_eq!(p(KeyCode::Char('s')), None);
    }

    #[test]
    fn any_key_closes_help() {
        assert_eq!(
            action_for(k(KeyCode::Char('x')), InputMode::Help),
            Some(Action::CloseHelp)
        );
    }

    #[test]
    fn saved_workspace_questions_take_yes_no_and_typing() {
        let c = |code| action_for(k(code), InputMode::SavedConfirm);
        assert_eq!(
            c(KeyCode::Char('y')),
            Some(Action::SavedPrompt(SavedPromptInput::Yes))
        );
        assert_eq!(
            c(KeyCode::Char('n')),
            Some(Action::SavedPrompt(SavedPromptInput::Cancel))
        );
        assert_eq!(c(KeyCode::Char('x')), None, "no accidental letters");
        // While a directory is typed, letters type (including y and n).
        let t = |code| action_for(k(code), InputMode::SavedInput);
        assert_eq!(
            t(KeyCode::Char('y')),
            Some(Action::SavedPrompt(SavedPromptInput::Char('y')))
        );
        assert_eq!(
            t(KeyCode::Esc),
            Some(Action::SavedPrompt(SavedPromptInput::Cancel))
        );
    }
}
