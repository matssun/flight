// SPDX-License-Identifier: MIT

use crate::{Tmux, TmuxError, TmuxRunner};

/// One tmux client attached to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    /// `#{client_name}`, the value `switch-client -c` takes.
    pub name: String,
    /// A control-mode client (Flight's own observer, an editor integration) is not a terminal
    /// a person is looking at.
    pub control: bool,
}

impl<R: TmuxRunner> Tmux<R> {
    /// The clients attached to `session` (a name or `$id`).
    pub fn list_clients(&self, session: &str) -> Result<Vec<ClientInfo>, TmuxError> {
        let out = self.runner().run(&[
            "list-clients",
            "-t",
            session,
            "-F",
            "#{client_name}\t#{client_control_mode}",
        ])?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                let (name, control) = line.split_once('\t')?;
                (!name.is_empty()).then(|| ClientInfo {
                    name: name.to_owned(),
                    control: control != "0",
                })
            })
            .collect())
    }

    /// The id (`$N`) of the session that holds `pane`. Does not depend on any client.
    pub fn session_id_of_pane(&self, pane: &str) -> Result<String, TmuxError> {
        let out = self
            .runner()
            .run(&["display-message", "-p", "-t", pane, "#{session_id}"])?;
        Ok(out.stdout.trim().to_owned())
    }

    /// Point one named client at `target` (a pane, window or session).
    pub fn switch_named_client(&self, client: &str, target: &str) -> Result<(), TmuxError> {
        self.runner()
            .run(&["switch-client", "-c", client, "-t", target])
            .map(drop)
    }
}
