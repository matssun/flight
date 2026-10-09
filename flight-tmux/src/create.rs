// SPDX-License-Identifier: MIT

use crate::{
    CreateError, Launch, NewSession, Tmux, TmuxError, TmuxRunner, SURFACE_ID_OPTION,
    SURFACE_OPTION, WORKSPACE_OPTION,
};
use std::thread::sleep;
use std::time::Duration;

/// The session option Flight sets on every session it creates. A node publishes the panes of
/// such a session even when they run no known agent (a plain shell), so what a person created
/// from Flight shows up in Flight.
pub const FLIGHT_SESSION_OPTION: &str = "@flight_session";

/// How long a new session's program gets to fail before the create is called successful.
const LAUNCH_CHECKS: u32 = 4;
const LAUNCH_CHECK_EVERY: Duration = Duration::from_millis(50);

impl<R: TmuxRunner> Tmux<R> {
    /// Create a detached session, mark it as Flight's and confirm its program started.
    /// Returns the session's id (`$N`).
    ///
    /// Everything after the creation addresses the session by that id, so nothing here can
    /// touch another session; if any step after the creation fails the new session (and only
    /// it) is killed before the error is returned. An existing session of the same name is
    /// never modified: that is [`CreateError::AlreadyExists`].
    pub fn create_session(&self, spec: &NewSession) -> Result<String, CreateError> {
        if self.has_session(&spec.name) {
            return Err(CreateError::AlreadyExists);
        }
        let id = self.start_session(spec)?;
        match self.settle(&id) {
            Ok(()) => Ok(id),
            Err(e) => {
                // Best effort: the session may be gone already, which is the goal.
                let _ = self.runner().run(&["kill-session", "-t", &id]);
                Err(e)
            }
        }
    }

    fn start_session(&self, spec: &NewSession) -> Result<String, CreateError> {
        let mut args: Vec<String> = ["new-session", "-d", "-P", "-F", "#{session_id}", "-s"]
            .map(str::to_owned)
            .into();
        args.extend([spec.name.clone(), "-c".to_owned(), spec.dir.clone()]);
        if let Some(mark) = &spec.mark {
            args.extend(["-n".to_owned(), mark.kind.as_str().to_owned()]);
        }
        if let Launch::Program { argv } = &spec.launch {
            args.extend(argv.iter().cloned());
        }
        if let Some(mark) = &spec.mark {
            // The identity is written by the same tmux invocation that creates the session, so
            // no observer can see the session without it.
            for (flag, option, value) in [
                (None, FLIGHT_SESSION_OPTION, "1"),
                (None, WORKSPACE_OPTION, mark.workspace_id.as_str()),
                (Some("-w"), SURFACE_OPTION, mark.kind.as_str()),
                (Some("-w"), SURFACE_ID_OPTION, mark.surface_id.as_str()),
            ] {
                args.push(";".to_owned());
                args.push("set-option".to_owned());
                args.extend(flag.map(str::to_owned));
                args.extend([option.to_owned(), value.to_owned()]);
            }
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.runner().run(&args) {
            Ok(out) => {
                let id = out.stdout.trim().to_owned();
                if id.starts_with('$') {
                    Ok(id)
                } else {
                    Err(TmuxError::Unparseable(id).into())
                }
            }
            // Lost a race with another creator of the same name.
            Err(TmuxError::Failed { stderr, .. }) if stderr.contains("duplicate session") => {
                Err(CreateError::AlreadyExists)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Mark the new session and watch it live through its first moments.
    fn settle(&self, id: &str) -> Result<(), CreateError> {
        if let Err(e) = self
            .runner()
            .run(&["set-option", "-t", id, FLIGHT_SESSION_OPTION, "1"])
        {
            // A program that ends at once takes its session with it before the mark lands:
            // that is an exited program, not a tmux failure.
            if self.runner().run(&["has-session", "-t", id]).is_err() {
                return Err(CreateError::Exited);
            }
            return Err(e.into());
        }
        for check in 0..LAUNCH_CHECKS {
            if check > 0 {
                sleep(LAUNCH_CHECK_EVERY);
            }
            // An id needs no `=`. A dead session (or the whole server, when it was the only
            // session) fails the lookup.
            if self.runner().run(&["has-session", "-t", id]).is_err() {
                return Err(CreateError::Exited);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
