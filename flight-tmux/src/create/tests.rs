// SPDX-License-Identifier: MIT

use super::*;
use crate::TmuxOutput;
use std::cell::RefCell;

/// A scripted tmux that records every call. By default no session named `api` exists, a
/// created session has id `$7`, and every other call succeeds.
#[derive(Default)]
struct Script {
    calls: RefCell<Vec<Vec<String>>>,
    name_taken: bool,
    /// Calls whose first argument is the key fail with the value as stderr.
    fail: Vec<(&'static str, &'static str)>,
}

impl Script {
    fn failing(verb: &'static str, stderr: &'static str) -> Self {
        Self {
            fail: vec![(verb, stderr)],
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }
}

fn refused(stderr: &str) -> TmuxError {
    TmuxError::Failed {
        code: Some(1),
        stderr: stderr.to_owned(),
    }
}

impl TmuxRunner for &Script {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        let by_name = args[0] == "has-session" && args[2].starts_with('=');
        if by_name && !self.name_taken {
            return Err(refused("can't find session"));
        }
        if let Some((_, stderr)) = self.fail.iter().find(|(verb, _)| *verb == args[0]) {
            return Err(refused(stderr));
        }
        let stdout = if args[0] == "new-session" { "$7\n" } else { "" };
        Ok(TmuxOutput {
            stdout: stdout.to_owned(),
        })
    }
}

fn spec(launch: Launch) -> NewSession {
    NewSession {
        name: "api".to_owned(),
        dir: "/work".to_owned(),
        launch,
        mark: None,
    }
}

#[test]
fn a_session_is_created_marked_and_checked_by_id() {
    let script = Script::default();
    let id = Tmux::with_runner(&script)
        .create_session(&spec(Launch::Program {
            argv: vec!["/bin/claude".into()],
        }))
        .unwrap();
    assert_eq!(id, "$7");
    let calls = script.calls();
    assert_eq!(calls[0], ["has-session", "-t", "=api"]);
    assert_eq!(
        calls[1],
        [
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{session_id}",
            "-s",
            "api",
            "-c",
            "/work",
            "/bin/claude"
        ]
    );
    assert_eq!(
        calls[2],
        ["set-option", "-t", "$7", FLIGHT_SESSION_OPTION, "1"]
    );
    assert_eq!(calls[3], ["has-session", "-t", "$7"]);
}

#[test]
fn the_default_shell_passes_no_program() {
    let script = Script::default();
    Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap();
    assert_eq!(script.calls()[1].last().map(String::as_str), Some("/work"));
}

#[test]
fn an_existing_name_is_refused_before_anything_is_created() {
    let script = Script {
        name_taken: true,
        ..Script::default()
    };
    let err = Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap_err();
    assert!(matches!(err, CreateError::AlreadyExists), "{err}");
    assert_eq!(script.calls().len(), 1);
}

#[test]
fn losing_a_race_for_the_name_is_already_exists_too() {
    let script = Script::failing("new-session", "duplicate session: api");
    let err = Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap_err();
    assert!(matches!(err, CreateError::AlreadyExists), "{err}");
}

/// A failure after the session exists kills that session by its id: never by name, so a
/// session that happens to share the name is not at risk.
#[test]
fn a_failure_after_creation_kills_only_the_new_session_by_id() {
    let script = Script::failing("set-option", "boom");
    let err = Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap_err();
    assert!(matches!(err, CreateError::Tmux(_)), "{err}");
    let calls = script.calls();
    assert_eq!(calls.last().unwrap(), &["kill-session", "-t", "$7"]);
}

#[test]
fn a_session_that_vanishes_is_reported_and_cleaned_up_by_id() {
    let script = Script::failing("has-session", "can't find session");
    let err = Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap_err();
    assert!(matches!(err, CreateError::Exited), "{err}");
    assert_eq!(
        script.calls().last().unwrap(),
        &["kill-session", "-t", "$7"]
    );
}

/// The program may end before the mark is set; the session is then already gone, and the
/// answer is "it exited", not a tmux error.
#[test]
fn a_program_that_ends_before_the_mark_lands_is_an_exit_not_a_tmux_error() {
    let script = Script {
        fail: vec![
            ("set-option", "can't find session: $7"),
            ("has-session", "can't find session: $7"),
        ],
        ..Script::default()
    };
    let err = Tmux::with_runner(&script)
        .create_session(&spec(Launch::DefaultShell))
        .unwrap_err();
    assert!(matches!(err, CreateError::Exited), "{err}");
}
