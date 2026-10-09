// SPDX-License-Identifier: MIT

use crate::ConnId;
use flight_proto::TERMINAL_ID_LEN;
use flight_state::HostId;
use std::collections::BTreeMap;

/// 128 random bits, minted here and nowhere else.
pub type TerminalId = [u8; TERMINAL_ID_LEN];

/// Which end of a terminal is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ui,
    Node,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The node has been asked; it has not answered.
    Opening,
    /// The node answered; both ends have until `deadline` to attach.
    Opened,
}

/// Everything an id is bound to. Nothing a peer says later can change it.
#[derive(Debug, Clone)]
pub(crate) struct Terminal {
    /// The authenticated identity of the UI that asked.
    pub(crate) ui_identity: String,
    pub(crate) host: HostId,
    pub(crate) conn: ConnId,
    /// The pane, as `server/pane`, for "one terminal per UI and pane".
    pub(crate) pane: String,
    pub(crate) pid: u32,
    pub(crate) phase: Phase,
    pub(crate) deadline: u64,
    pub(crate) ui_attached: bool,
    pub(crate) node_attached: bool,
    /// The UI has said goodbye; the node is reading what is left. A closing terminal is not
    /// replaced by a new one for the same pane (it is allowed to finish) and does not count
    /// against the UI's limit, but still counts against the node's and the total.
    pub(crate) closing: bool,
}

/// The live terminals, by id. An id that is not here is dead: ids are never reissued.
#[derive(Default)]
pub(crate) struct Terminals {
    entries: BTreeMap<TerminalId, Terminal>,
}

impl Terminals {
    pub(crate) fn insert(&mut self, id: TerminalId, terminal: Terminal) {
        self.entries.insert(id, terminal);
    }

    pub(crate) fn get(&self, id: &TerminalId) -> Option<&Terminal> {
        self.entries.get(id)
    }

    pub(crate) fn get_mut(&mut self, id: &TerminalId) -> Option<&mut Terminal> {
        self.entries.get_mut(id)
    }

    pub(crate) fn remove(&mut self, id: &TerminalId) -> Option<Terminal> {
        self.entries.remove(id)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn count_where(&self, pred: impl Fn(&Terminal) -> bool) -> usize {
        self.entries.values().filter(|t| pred(t)).count()
    }

    /// Ids of the terminals matching `pred`.
    pub(crate) fn ids_where(&self, pred: impl Fn(&Terminal) -> bool) -> Vec<TerminalId> {
        self.entries
            .iter()
            .filter(|(_, t)| pred(t))
            .map(|(id, _)| *id)
            .collect()
    }
}
