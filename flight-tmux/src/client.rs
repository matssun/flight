// SPDX-License-Identifier: MIT

use crate::{
    parse_panes_output, PaneInfo, SystemRunner, TmuxEndpoint, TmuxError, TmuxRunner, PANE_FORMAT,
};

/// High-level tmux operations over a [`TmuxRunner`].
#[derive(Debug, Clone)]
pub struct Tmux<R = SystemRunner> {
    runner: R,
}

impl Tmux<SystemRunner> {
    pub fn new(endpoint: TmuxEndpoint) -> Self {
        Self {
            runner: SystemRunner::new(endpoint),
        }
    }
}

impl<R: TmuxRunner> Tmux<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }

    /// The underlying runner, for callers that need to issue their own tmux command.
    pub fn runner(&self) -> &R {
        &self.runner
    }

    /// Every pane on the server. `Err(Failed)` usually means no server is running.
    pub fn list_panes(&self) -> Result<Vec<PaneInfo>, TmuxError> {
        let out = self.runner.run(&["list-panes", "-a", "-F", PANE_FORMAT])?;
        let panes = parse_panes_output(&out.stdout);
        if panes.is_empty() {
            // Panes were listed but none could be read: say so rather than report "no panes".
            if let Some(line) = out.stdout.lines().find(|l| !l.trim().is_empty()) {
                return Err(TmuxError::Unparseable(line.chars().take(80).collect()));
            }
        }
        Ok(panes)
    }

    /// Visible text of a pane. `ansi` keeps escape sequences (`-e`); `lines` limits to the
    /// last N lines of the visible screen.
    pub fn capture_pane(
        &self,
        pane: &str,
        ansi: bool,
        lines: Option<u32>,
    ) -> Result<String, TmuxError> {
        let start = lines.map(|n| format!("-{n}"));
        let mut args = vec!["capture-pane", "-p", "-t", pane];
        if ansi {
            args.push("-e");
        }
        if let Some(s) = start.as_deref() {
            args.extend(["-S", s]);
        }
        Ok(self.runner.run(&args)?.stdout)
    }

    pub fn has_session(&self, name: &str) -> bool {
        self.runner
            .run(&["has-session", "-t", &exact(name)])
            .is_ok()
    }

    /// Switch the current client to `target` (session, window or pane).
    pub fn switch_client(&self, target: &str) -> Result<(), TmuxError> {
        self.runner.run(&["switch-client", "-t", target]).map(drop)
    }

    pub fn kill_pane(&self, pane: &str) -> Result<(), TmuxError> {
        self.runner.run(&["kill-pane", "-t", pane]).map(drop)
    }

    pub fn kill_session(&self, name: &str) -> Result<(), TmuxError> {
        self.runner
            .run(&["kill-session", "-t", &exact(name)])
            .map(drop)
    }

    /// Create a detached session rooted at `dir` running `command` (a shell command line).
    pub fn new_session_running(
        &self,
        name: &str,
        dir: &str,
        command: &str,
    ) -> Result<(), TmuxError> {
        self.runner
            .run(&["new-session", "-d", "-s", name, "-c", dir, command])
            .map(drop)
    }

    /// Stop the whole server, killing every session on it.
    pub fn kill_server(&self) -> Result<(), TmuxError> {
        self.runner.run(&["kill-server"]).map(drop)
    }

    /// Create a detached session rooted at `dir`.
    pub fn new_session(&self, name: &str, dir: &str) -> Result<(), TmuxError> {
        self.runner
            .run(&["new-session", "-d", "-s", name, "-c", dir])
            .map(drop)
    }
}

/// `=name` makes tmux match the session name exactly instead of by prefix.
fn exact(name: &str) -> String {
    format!("={name}")
}

#[cfg(test)]
mod tests;
