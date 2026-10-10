// SPDX-License-Identifier: MIT

use crate::switch::{
    detect_placement, plan_switch, AttachCommand, RemoteOps, SwitchError, SwitchPlan, SwitchTarget,
    TmuxEnv, UiContext,
};
use flight_state::HostId;
use flight_tmux::{GuardError, Tmux, TmuxEndpoint};

/// How a switch ended, when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented {
    /// The terminal showing the dashboard now shows the pane; the dashboard can exit.
    ClientMoved,
    /// The dashboard exits, then this local attach takes over the terminal.
    Attach(AttachCommand),
    /// A terminal onto a remote pane was opened; the dashboard exits, shows it, and returns.
    Terminal(Vec<u8>),
}

/// Plans a switch and runs it in two explicit stages: reveal (select the window and pane),
/// then present (show them in a terminal).
#[derive(Debug, Clone, Default)]
pub struct Switcher {
    local_host: Option<HostId>,
}

impl Switcher {
    pub fn new(local_host: Option<HostId>) -> Self {
        Self { local_host }
    }

    /// `remote` carries a switch to a pane on another machine through the orchestrator:
    /// first a guarded reveal, then a guarded terminal. It is used only for a remote pane, and
    /// only after planning has accepted the switch.
    pub fn switch(
        &self,
        target: &SwitchTarget,
        env: &TmuxEnv,
        remote: &mut dyn RemoteOps,
    ) -> Result<Presented, SwitchError> {
        let placement = detect_placement(env);
        let ctx = UiContext {
            local_host: self.local_host.as_ref(),
            placement: &placement,
        };
        match plan_switch(target, &ctx)? {
            SwitchPlan::LocalClient { client, target } => {
                let tmux = local_tmux(&target)?;
                reveal_local(&tmux, &target)?;
                tmux.switch_named_client(&client, target.pane.pane.as_str())
                    .map_err(|e| SwitchError::Present(e.to_string()))?;
                Ok(Presented::ClientMoved)
            }
            SwitchPlan::LocalAttach { server, target } => {
                let tmux = local_tmux(&target)?;
                reveal_local(&tmux, &target)?;
                let endpoint = TmuxEndpoint::named(&server)
                    .map_err(|e| SwitchError::Present(e.to_string()))?;
                let session = format!("={}", target.session);
                let args = flight_tmux::tmux_args(&endpoint, &["attach-session", "-t", &session]);
                Ok(Presented::Attach(AttachCommand {
                    program: "tmux".to_owned(),
                    args,
                }))
            }
            SwitchPlan::RemoteTerminal { target, .. } => {
                remote
                    .reveal(&target.pane, target.pid)
                    .map_err(SwitchError::Reveal)?;
                // The pane is selected; from here a failure is a failure to show it.
                let id = remote
                    .open_terminal(&target.pane, target.pid)
                    .map_err(|e| SwitchError::Present(format!("cannot open a terminal: {e}")))?;
                Ok(Presented::Terminal(id))
            }
        }
    }
}

fn local_tmux(target: &SwitchTarget) -> Result<Tmux, SwitchError> {
    TmuxEndpoint::named(target.pane.server.as_str())
        .map(Tmux::new)
        .map_err(|e| SwitchError::Reveal(e.to_string()))
}

/// The local twin of the node's guarded reveal: act only on the process the user saw.
fn reveal_local(tmux: &Tmux, target: &SwitchTarget) -> Result<(), SwitchError> {
    let current = tmux
        .guarded_pane(target.pane.pane.as_str(), target.pid)
        .map_err(|e| match e {
            GuardError::Tmux(e) => SwitchError::Reveal(e.to_string()),
            GuardError::Missing => SwitchError::Reveal("the pane no longer exists".to_owned()),
            GuardError::Changed { .. } => {
                SwitchError::Reveal("the pane changed since it was listed; refresh".to_owned())
            }
        })?;
    tmux.reveal_pane(&current.window_id, target.pane.pane.as_str())
        .map_err(|e| SwitchError::Reveal(e.to_string()))
}
