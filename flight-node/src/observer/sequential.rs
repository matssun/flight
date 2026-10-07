// SPDX-License-Identifier: MIT

use super::PaneObserver;
use crate::{Round, TmuxServers};
use std::sync::Arc;

/// One plain tmux call per command (a list, then a capture per agent pane). The reference
/// path: simple, stateless, and the fallback of every other observer.
pub struct SequentialObserver {
    servers: Arc<TmuxServers>,
}

impl SequentialObserver {
    pub fn new(servers: Arc<TmuxServers>) -> Self {
        Self { servers }
    }
}

impl PaneObserver for SequentialObserver {
    fn observe(&mut self, now: u64) -> Vec<Round> {
        self.servers.observe(now)
    }
}
