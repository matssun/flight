// SPDX-License-Identifier: MIT

use crate::shell_quote::shell_quote;
use crate::HostError;
use flight_tmux::{TmuxEndpoint, TmuxError, TmuxOutput, TmuxRunner};
use std::process::Command;

/// Seconds before ssh gives up connecting, so an unreachable host fails fast.
const DEFAULT_CONNECT_TIMEOUT_SECS: u32 = 5;

/// Runs tmux on another machine as `ssh <alias> tmux -L <name> ...`. Deliberately boring: it
/// consumes an SSH host alias and leaves keys, hostnames, ProxyJump, ControlMaster and host
/// verification to the user's OpenSSH config. It adds only per-call safety: `BatchMode` so
/// it can never hang on a password prompt, and a connect timeout.
#[derive(Debug, Clone)]
pub struct SshRunner {
    alias: String,
    endpoint: TmuxEndpoint,
    connect_timeout_secs: u32,
}

impl SshRunner {
    /// `alias` must be non-empty, contain no whitespace, and not start with `-` (it would
    /// be read as an ssh option).
    pub fn new(alias: &str, endpoint: TmuxEndpoint) -> Result<Self, HostError> {
        if alias.is_empty() || alias.starts_with('-') || alias.chars().any(char::is_whitespace) {
            return Err(HostError::InvalidConfig(format!("ssh alias {alias:?}")));
        }
        Ok(Self {
            alias: alias.to_owned(),
            endpoint,
            connect_timeout_secs: DEFAULT_CONNECT_TIMEOUT_SECS,
        })
    }

    /// The full `ssh` argument list (without the program name) for a tmux invocation.
    pub fn ssh_args(&self, tmux_args: &[&str]) -> Vec<String> {
        let remote = std::iter::once("tmux".to_owned())
            .chain(self.endpoint.args())
            .chain(tmux_args.iter().map(|a| (*a).to_owned()))
            .map(|a| shell_quote(&a))
            .collect::<Vec<_>>()
            .join(" ");
        vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            format!("ConnectTimeout={}", self.connect_timeout_secs),
            "--".into(),
            self.alias.clone(),
            remote,
        ]
    }
}

impl TmuxRunner for SshRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let out = Command::new("ssh")
            .args(self.ssh_args(args))
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

    fn runner() -> SshRunner {
        SshRunner::new("mini-2", TmuxEndpoint::named("flight").unwrap()).unwrap()
    }

    #[test]
    fn builds_a_quoted_remote_command() {
        let args = runner().ssh_args(&["has-session", "-t", "=api"]);
        assert_eq!(
            args[..6],
            [
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                "--",
                "mini-2"
            ]
        );
        assert_eq!(args[6], "'tmux' '-L' 'flight' 'has-session' '-t' '=api'");
    }

    #[test]
    fn quotes_format_strings_so_the_remote_shell_cannot_mangle_them() {
        let args = runner().ssh_args(&["list-panes", "-F", "#{pane_id}\t#{pane_title}"]);
        assert!(args[6].ends_with("'#{pane_id}\t#{pane_title}'"));
    }

    #[test]
    fn rejects_aliases_that_could_inject_options_or_commands() {
        let ep = || TmuxEndpoint::named("flight").unwrap();
        for bad in ["", "-oProxyCommand=x", "a b", "a\tb"] {
            assert!(SshRunner::new(bad, ep()).is_err(), "{bad:?}");
        }
    }
}
