// SPDX-License-Identifier: MIT

use crate::{TmuxEndpoint, TmuxError};
use std::process::Command;

/// Captured result of one tmux invocation.
#[derive(Debug, Clone)]
pub struct TmuxOutput {
    pub stdout: String,
}

/// Seam over process spawning so `Tmux` is testable without a tmux server.
pub trait TmuxRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError>;
}

/// Lets a registry hold runners of different kinds behind one type.
impl<R: TmuxRunner + ?Sized> TmuxRunner for Box<R> {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        (**self).run(args)
    }
}

/// The arguments of one tmux invocation: `-u` first, then the endpoint, then `args`.
///
/// `-u` forces UTF-8 output. Without it tmux judges the locale from the environment, and a
/// process with no `LANG`/`LC_*` (a launchd or systemd service, a non-login ssh command) gets
/// its `-F` field separators and every non-ASCII character in captured screens rewritten to
/// `_`: the pane list then parses as empty and screen classification loses its glyphs.
pub fn tmux_args(endpoint: &TmuxEndpoint, args: &[&str]) -> Vec<String> {
    std::iter::once("-u".to_owned())
        .chain(endpoint.args())
        .chain(args.iter().map(|a| (*a).to_owned()))
        .collect()
}

/// Runs the real `tmux` binary against one explicit endpoint.
#[derive(Debug, Clone)]
pub struct SystemRunner {
    endpoint: TmuxEndpoint,
}

impl SystemRunner {
    pub fn new(endpoint: TmuxEndpoint) -> Self {
        Self { endpoint }
    }
}

impl TmuxRunner for SystemRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let out = Command::new("tmux")
            .args(tmux_args(&self.endpoint, args))
            .output()
            .map_err(TmuxError::Spawn)?;
        if out.status.success() {
            Ok(TmuxOutput {
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            })
        } else {
            Err(TmuxError::Failed {
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_is_forced_before_the_endpoint_and_the_command() {
        let endpoint = TmuxEndpoint::named("flight").unwrap();
        assert_eq!(
            tmux_args(&endpoint, &["list-panes", "-a"]),
            ["-u", "-L", "flight", "list-panes", "-a"]
        );
    }
}
