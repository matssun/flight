// SPDX-License-Identifier: MIT

use super::lists::workspaces;
use crate::snapshot::{HostHealth, UiSnapshot};
use flight_state::AgentState;
use std::collections::BTreeSet;

/// Counts for the header strip, over every workspace regardless of the search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Waiting for approval or asking a question.
    pub need_you: usize,
    /// Finished; the next move is the user's.
    pub ready: usize,
    pub working: usize,
    pub idle: usize,
    pub shell: usize,
    pub down: usize,
    pub hosts: usize,
    /// Hosts the dashboard currently has a live link to.
    pub hosts_up: usize,
}

impl Summary {
    pub fn of(s: &UiSnapshot) -> Self {
        let mut out = Self::default();
        let mut all = BTreeSet::new();
        let mut up = BTreeSet::new();
        for h in &s.hosts {
            all.insert(&h.host);
            if matches!(h.health, HostHealth::Online | HostHealth::NoServer) {
                up.insert(&h.host);
            }
        }
        // One count per workspace, by its state: a workspace's shell is not another session.
        for w in workspaces(s, "") {
            let slot = match w.state() {
                AgentState::Permit | AgentState::Question => &mut out.need_you,
                AgentState::Done => &mut out.ready,
                AgentState::Busy => &mut out.working,
                AgentState::Idle => &mut out.idle,
                AgentState::Shell => &mut out.shell,
                AgentState::Down => &mut out.down,
            };
            *slot = slot.saturating_add(1);
        }
        out.hosts = all.len();
        out.hosts_up = up.len();
        out
    }

    pub fn workspaces(&self) -> usize {
        [
            self.need_you,
            self.ready,
            self.working,
            self.idle,
            self.shell,
            self.down,
        ]
        .iter()
        .fold(0usize, |a, n| a.saturating_add(*n))
    }
}
