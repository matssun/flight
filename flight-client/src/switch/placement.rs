// SPDX-License-Identifier: MIT

use flight_tmux::{Tmux, TmuxEndpoint};
use std::path::Path;

/// The environment tmux gives the processes it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxEnv {
    /// `$TMUX`: `<socket path>,<server pid>,<session number>`.
    pub tmux: Option<String>,
    /// `$TMUX_PANE`: set in an ordinary pane, empty inside `display-popup`.
    pub tmux_pane: Option<String>,
}

impl TmuxEnv {
    pub fn from_process() -> Self {
        let get = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            tmux: get("TMUX"),
            tmux_pane: get("TMUX_PANE"),
        }
    }
}

/// Where the dashboard itself runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiPlacement {
    OutsideTmux,
    InsideTmux {
        /// The `-L` name of the server, when its socket is a named one in tmux's own socket
        /// directory; `None` for anything else (an explicit `-S` path), which never matches.
        server: Option<String>,
        /// The terminals attached to the dashboard's session, control clients excluded.
        /// Empty when they could not be determined.
        clients: Vec<String>,
    },
}

/// `<dir>/tmux-<uid>/<name>` is the socket of `tmux -L <name>`.
fn named_server(socket: &Path) -> Option<String> {
    let dir = socket.parent()?.file_name()?.to_str()?;
    dir.starts_with("tmux-")
        .then(|| socket.file_name()?.to_str().map(str::to_owned))?
}

/// The session number in `$TMUX` as a tmux target (`$3`).
fn session_from_tmux_var(var: &str) -> Option<String> {
    let number = var.rsplit(',').next()?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then(|| format!("${number}"))
}

/// Find the dashboard's session, then the terminals attached to it. The session comes from
/// the pane when there is one (it does not depend on any client) and from `$TMUX` inside a
/// popup. "Which client am I" is deliberately not asked of tmux: with two terminals on one
/// session it answers with the most recently active one, not the one that ran the command.
pub fn detect_placement(env: &TmuxEnv) -> UiPlacement {
    let Some(var) = env.tmux.as_deref() else {
        return UiPlacement::OutsideTmux;
    };
    let socket = var.rsplitn(3, ',').nth(2).unwrap_or(var);
    let tmux = Tmux::new(TmuxEndpoint::Path(socket.into()));
    let session = match env.tmux_pane.as_deref() {
        Some(pane) => tmux.session_id_of_pane(pane).ok(),
        None => session_from_tmux_var(var),
    };
    let clients = session
        .and_then(|s| tmux.list_clients(&s).ok())
        .map(|all| {
            all.into_iter()
                .filter(|c| !c.control)
                .map(|c| c.name)
                .collect()
        })
        .unwrap_or_default();
    UiPlacement::InsideTmux {
        server: named_server(Path::new(socket)),
        clients,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_server_is_recognised_by_its_socket() {
        assert_eq!(
            named_server(Path::new("/private/tmp/tmux-501/flight")),
            Some("flight".to_owned())
        );
        assert_eq!(named_server(Path::new("/home/me/my-socket")), None);
    }

    #[test]
    fn the_session_comes_out_of_the_tmux_variable() {
        assert_eq!(
            session_from_tmux_var("/tmp/tmux-501/flight,1234,7"),
            Some("$7".to_owned())
        );
        assert_eq!(session_from_tmux_var("/tmp/x,1234,"), None);
        assert_eq!(session_from_tmux_var("/tmp/x,1234,a b"), None);
    }

    #[test]
    fn outside_tmux_there_is_no_placement_to_find() {
        assert_eq!(
            detect_placement(&TmuxEnv::default()),
            UiPlacement::OutsideTmux
        );
    }
}
