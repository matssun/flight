// SPDX-License-Identifier: MIT

//! Host-routed operations. Each resolves (host, server), runs one `flight-tmux` call, and
//! classifies any failure for the host's transport.

use crate::panes_outcome::{HostPane, PanesOutcome};
use crate::probe::probe;
use crate::{HostError, HostRegistry, HostStatus};
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use flight_tmux::Tmux;

impl HostRegistry {
    pub fn status(&self, host: &HostId, server: &ServerId) -> Result<HostStatus, HostError> {
        let (h, s) = self.server(host, server)?;
        Ok(probe(&h.transport, s.tmux.runner()))
    }

    pub fn list_panes(&self, host: &HostId, server: &ServerId) -> Result<Vec<HostPane>, HostError> {
        let panes = self.call(host, server, Tmux::list_panes)?;
        Ok(panes
            .into_iter()
            .map(|info| HostPane {
                pane_ref: PaneRef {
                    host: host.clone(),
                    server: server.clone(),
                    pane: PaneId::new(&info.pane_id),
                },
                info,
            })
            .collect())
    }

    /// Every pane on every registered endpoint, one outcome each, so a down host shows up
    /// as an error beside the hosts that answered.
    pub fn list_all_panes(&self) -> Vec<PanesOutcome> {
        let mut out = Vec::new();
        for (host, h) in &self.hosts {
            for server in h.servers.keys() {
                out.push(PanesOutcome {
                    host: host.clone(),
                    server: server.clone(),
                    result: self.list_panes(host, server),
                });
            }
        }
        out
    }

    pub fn capture_pane(
        &self,
        pane: &PaneRef,
        ansi: bool,
        lines: Option<u32>,
    ) -> Result<String, HostError> {
        self.call(&pane.host, &pane.server, |t| {
            t.capture_pane(pane.pane.as_str(), ansi, lines)
        })
    }

    pub fn kill_pane(&self, pane: &PaneRef) -> Result<(), HostError> {
        self.call(&pane.host, &pane.server, |t| {
            t.kill_pane(pane.pane.as_str())
        })
    }

    pub fn has_session(
        &self,
        host: &HostId,
        server: &ServerId,
        name: &str,
    ) -> Result<bool, HostError> {
        let (_, s) = self.server(host, server)?;
        Ok(s.tmux.has_session(name))
    }

    pub fn new_session(
        &self,
        host: &HostId,
        server: &ServerId,
        name: &str,
        dir: &str,
    ) -> Result<(), HostError> {
        self.call(host, server, |t| t.new_session(name, dir))
    }

    pub fn kill_server(&self, host: &HostId, server: &ServerId) -> Result<(), HostError> {
        self.call(host, server, Tmux::kill_server)
    }
}
