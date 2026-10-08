// SPDX-License-Identifier: MIT

use crate::{ConnId, TerminalId, UiId};
use std::collections::BTreeMap;

/// A routed request awaiting its node's response.
#[derive(Debug, Clone)]
pub(crate) struct PendingRequest {
    pub(crate) conn: ConnId,
    pub(crate) ui: UiId,
    pub(crate) ui_request_id: u64,
    pub(crate) deadline: u64,
    /// Set for an `OpenTerminal`: the id its answer must carry.
    pub(crate) terminal: Option<TerminalId>,
}

/// Requests in flight. Never replayed: if the node goes away they fail, they are not queued.
#[derive(Default)]
pub(crate) struct Pending {
    next_id: u64,
    requests: BTreeMap<u64, PendingRequest>,
}

impl Pending {
    /// Registers a request and returns the id to send to the node.
    pub(crate) fn add(&mut self, req: PendingRequest) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.requests.insert(self.next_id, req);
        self.next_id
    }

    /// The pending request answered by `request_id` — only if it was asked of `conn`.
    pub(crate) fn complete(&mut self, conn: ConnId, request_id: u64) -> Option<PendingRequest> {
        match self.requests.get(&request_id) {
            Some(r) if r.conn == conn => self.requests.remove(&request_id),
            _ => None,
        }
    }

    pub(crate) fn fail_conn(&mut self, conn: ConnId) -> Vec<PendingRequest> {
        self.take_where(|r| r.conn == conn)
    }

    pub(crate) fn expired(&mut self, now: u64) -> Vec<PendingRequest> {
        self.take_where(|r| r.deadline <= now)
    }

    fn take_where(&mut self, pred: impl Fn(&PendingRequest) -> bool) -> Vec<PendingRequest> {
        let ids: Vec<u64> = self
            .requests
            .iter()
            .filter(|(_, r)| pred(r))
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.requests.remove(&id))
            .collect()
    }
}
