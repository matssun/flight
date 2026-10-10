// SPDX-License-Identifier: MIT

//! Acting on a pane or a terminal that a request named: capture, kill, reveal, and open a
//! terminal. Every action that targets a process is guarded: it happens to the process the
//! caller saw or not at all (`Tmux::guarded_pane`).

use super::{failed, TmuxServers};
use crate::{tmux_attach_command, ControlError, OpenedTerminal, TerminalProcess, TerminalSpec};
use flight_proto::ErrorKindCode;
use flight_state::{PaneId, ServerId};
use flight_tmux::{GuardError, Tmux};

impl TmuxServers {
    pub(super) fn pane_capture(
        &self,
        server: &ServerId,
        pane: &PaneId,
        lines: u32,
    ) -> Result<String, ControlError> {
        self.tmux(server)?
            .capture_pane(pane.as_str(), false, Some(lines))
            .map_err(failed)
    }

    pub(super) fn pane_kill(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        let tmux = self.tmux(server)?;
        // The request was issued against one process; only kill the pane if it still is it.
        tmux.guarded_pane(pane.as_str(), expected_pid)
            .map_err(|e| refused(e, pane))?;
        tmux.kill_pane(pane.as_str()).map_err(failed)
    }

    pub(super) fn pane_reveal(
        &self,
        server: &ServerId,
        pane: &PaneId,
        expected_pid: u32,
    ) -> Result<(), ControlError> {
        let tmux = self.tmux(server)?;
        // The request was issued against one process; only act if tmux still shows it.
        let current = tmux
            .guarded_pane(pane.as_str(), expected_pid)
            .map_err(|e| refused(e, pane))?;
        tmux.reveal_pane(&current.window_id, pane.as_str())
            .map_err(failed)
    }

    pub(super) fn terminal_open(
        &self,
        spec: &TerminalSpec,
    ) -> Result<OpenedTerminal, ControlError> {
        let endpoint = self.terminals.get(&spec.server).ok_or_else(|| {
            ControlError::new(
                ErrorKindCode::Unsupported,
                format!("terminals are not enabled for server {}", spec.server),
            )
        })?;
        // The same check as a reveal, before any PTY exists.
        let tmux = self.tmux(&spec.server)?;
        let current = tmux
            .guarded_pane(spec.pane.as_str(), spec.pid)
            .map_err(|e| refused(e, &spec.pane))?;
        // tmux repeats the check inside the command that attaches, to a view of its own.
        let view = flight_tmux::view_session_name(&spec.terminal_id);
        let args = tmux_attach_command(
            endpoint,
            spec.pane.as_str(),
            &current.window_id,
            spec.pid,
            &view,
        );
        let env = terminal_env(&spec.term);
        let mut opened = TerminalProcess::spawn("tmux", &args, &env, spec.cols, spec.rows)
            .map_err(|e| {
                ControlError::new(
                    ErrorKindCode::RemoteCommandFailed,
                    format!("cannot start a terminal: {e}"),
                )
            })?;
        if let Some(client_pid) = opened.process.process_id() {
            let tmux = Tmux::new(endpoint.clone());
            opened.redraw = Box::new(move || {
                let _ = tmux.refresh_client_of_pid(client_pid);
            });
        }
        // The view removes itself with its client; this covers a client that never attached.
        let tmux = Tmux::new(endpoint.clone());
        opened.cleanup = Box::new(move || {
            let _ = tmux.kill_session(&view);
        });
        Ok(opened)
    }
}

/// What a failed guarded lookup is, to the caller: the same fact, the same words, whichever
/// action asked.
fn refused(e: GuardError, pane: &PaneId) -> ControlError {
    match e {
        GuardError::Tmux(e) => failed(e),
        GuardError::Missing => {
            ControlError::new(ErrorKindCode::UnknownPane, format!("no pane {pane}"))
        }
        GuardError::Changed { .. } => ControlError::new(
            ErrorKindCode::PaneChanged,
            format!("pane {pane} is no longer the process this request targeted"),
        ),
    }
}

/// The whole environment of a terminal's tmux client: nothing is inherited but where to find
/// programs and the home directory. In particular no `TMUX` or `TMUX_PANE`.
fn terminal_env(term: &str) -> Vec<(String, String)> {
    let var = |name: &str, default: &str| {
        std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_owned())
    };
    vec![
        (
            "PATH".to_owned(),
            var("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"),
        ),
        ("HOME".to_owned(), var("HOME", "/")),
        ("TERM".to_owned(), term.to_owned()),
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
    ]
}
