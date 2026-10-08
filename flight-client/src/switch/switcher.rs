// SPDX-License-Identifier: MIT

use crate::switch::{
    detect_placement, plan_switch, Handoff, SshDestinations, SwitchError, SwitchPlan, SwitchTarget,
    TmuxEnv, UiContext,
};
use flight_control::SshRunner;
use flight_state::{HostId, PaneRef};
use flight_tmux::{Tmux, TmuxEndpoint};

/// How a switch ended, when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented {
    /// The terminal showing the dashboard now shows the pane; the dashboard can exit.
    ClientMoved,
    /// The dashboard exits, then this takes over the terminal.
    Attach(Handoff),
}

/// Plans a switch and runs it in two explicit stages: reveal (select the window and pane),
/// then present (show them in a terminal).
#[derive(Debug, Clone, Default)]
pub struct Switcher {
    local_host: Option<HostId>,
    ssh: SshDestinations,
}

fn on_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            std::fs::metadata(dir.join(program))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

impl Switcher {
    pub fn new(local_host: Option<HostId>, ssh: SshDestinations) -> Self {
        Self { local_host, ssh }
    }

    /// `reveal_remote` asks the pane's node, through the orchestrator, to select the pane
    /// if it still runs the process the user saw. It runs only for a remote pane, and only
    /// after planning has accepted the switch.
    pub fn switch(
        &self,
        target: &SwitchTarget,
        env: &TmuxEnv,
        reveal_remote: &mut dyn FnMut(&PaneRef, u32) -> Result<(), String>,
    ) -> Result<Presented, SwitchError> {
        let placement = detect_placement(env);
        let ctx = UiContext {
            local_host: self.local_host.as_ref(),
            placement: &placement,
            ssh: &self.ssh,
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
                Ok(Presented::Attach(Handoff {
                    program: "tmux".to_owned(),
                    args,
                }))
            }
            SwitchPlan::RemoteAttach {
                ssh_alias,
                server,
                target,
                ..
            } => {
                if !on_path("ssh") {
                    return Err(crate::switch::Refusal::MissingProgram("ssh").into());
                }
                let endpoint = TmuxEndpoint::named(&server)
                    .map_err(|e| SwitchError::Present(e.to_string()))?;
                let runner = SshRunner::new(&ssh_alias, endpoint)
                    .map_err(|e| SwitchError::Present(e.to_string()))?;
                reveal_remote(&target.pane, target.pid).map_err(SwitchError::Reveal)?;
                Ok(Presented::Attach(Handoff {
                    program: "ssh".to_owned(),
                    args: runner.attach_args(&target.session),
                }))
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
        .list_panes()
        .map_err(|e| SwitchError::Reveal(e.to_string()))?
        .into_iter()
        .find(|p| p.pane_id == target.pane.pane.as_str())
        .ok_or_else(|| SwitchError::Reveal("the pane no longer exists".to_owned()))?;
    if current.pane_pid != target.pid {
        return Err(SwitchError::Reveal(
            "the pane changed since it was listed; refresh".to_owned(),
        ));
    }
    tmux.reveal_pane(&current.window_id, target.pane.pane.as_str())
        .map_err(|e| SwitchError::Reveal(e.to_string()))
}
