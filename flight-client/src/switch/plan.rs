// SPDX-License-Identifier: MIT

use crate::switch::{Refusal, SshDestinations, UiPlacement};
use flight_control::SshRunner;
use flight_state::{HostId, PaneRef};
use flight_tmux::TmuxEndpoint;
use flight_ui::PaneView;

/// The pane the user asked for, exactly as the dashboard showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTarget {
    pub pane: PaneRef,
    /// The pane process the user saw; every step refuses if the pane is no longer it.
    pub pid: u32,
    pub session: String,
}

impl From<&PaneView> for SwitchTarget {
    fn from(p: &PaneView) -> Self {
        Self {
            pane: p.pane_ref.clone(),
            pid: p.pid,
            session: p.session.clone(),
        }
    }
}

/// What is known about the machine the dashboard runs on.
#[derive(Debug, Clone, Copy)]
pub struct UiContext<'a> {
    /// The node role on this machine, if any. A pane is local only if its node *is* this one.
    pub local_host: Option<&'a HostId>,
    pub placement: &'a UiPlacement,
    pub ssh: &'a SshDestinations,
}

/// What to do. Planning decides; it touches nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchPlan {
    /// Move the one terminal showing the dashboard to the pane.
    LocalClient {
        client: String,
        target: SwitchTarget,
    },
    /// Replace the dashboard with an attach to the pane's session.
    LocalAttach {
        server: String,
        target: SwitchTarget,
    },
    /// Reveal through the orchestrator, then replace the dashboard with an ssh attach.
    RemoteAttach {
        host: HostId,
        ssh_alias: String,
        server: String,
        target: SwitchTarget,
    },
}

fn is_pane_id(id: &str) -> bool {
    id.strip_prefix('%')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

pub fn plan_switch(target: &SwitchTarget, ctx: &UiContext<'_>) -> Result<SwitchPlan, Refusal> {
    if target.pid == 0 {
        return Err(Refusal::UnknownProcess);
    }
    if !is_pane_id(target.pane.pane.as_str()) {
        return Err(Refusal::BadTarget("pane id"));
    }
    let server = target.pane.server.as_str();
    if TmuxEndpoint::named(server).is_err() || server.starts_with('-') {
        return Err(Refusal::BadTarget("tmux server name"));
    }
    if target.session.is_empty() {
        return Err(Refusal::BadTarget("session name"));
    }
    let server = server.to_owned();
    let target = target.clone();
    if ctx.local_host != Some(&target.pane.host) {
        let host = target.pane.host.clone();
        let alias = ctx
            .ssh
            .get(&host)
            .ok_or_else(|| Refusal::NoSshDestination(host.clone()))?;
        SshRunner::check_alias(alias).map_err(|_| Refusal::UnsafeSshDestination(host.clone()))?;
        return Ok(SwitchPlan::RemoteAttach {
            host,
            ssh_alias: alias.to_owned(),
            server,
            target,
        });
    }
    match ctx.placement {
        UiPlacement::OutsideTmux => Ok(SwitchPlan::LocalAttach { server, target }),
        UiPlacement::InsideTmux {
            server: ui_server,
            clients,
        } => {
            if ui_server.as_deref() != Some(server.as_str()) {
                return Err(Refusal::OtherTmuxServer {
                    ui: ui_server.clone(),
                    pane: server,
                });
            }
            match clients.as_slice() {
                [] => Err(Refusal::NoClient),
                [only] => Ok(SwitchPlan::LocalClient {
                    client: only.clone(),
                    target,
                }),
                many => Err(Refusal::AmbiguousClients(many.len())),
            }
        }
    }
}
