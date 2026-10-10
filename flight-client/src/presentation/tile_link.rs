// SPDX-License-Identifier: MIT

use super::rebuild_budget::RebuildBudget;
use crate::screens::ScreenModel;
use crate::session::{Binding, ToRemote};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// One showing surface: its screen, and where its attachment is. A surface that is not showing
/// has none of this.
pub(super) struct TileLink {
    pub(super) model: ScreenModel,
    pub(super) state: LinkState,
    /// Incremented for each attachment of this surface; reports from older ones are ignored.
    pub(super) generation: u64,
    /// How often this screen may be rebuilt from its surface after the emulator failed.
    pub(super) budget: RebuildBudget,
}

pub(super) enum LinkState {
    /// Being attached. `attempts` counts the re-attachments made since the stream last worked.
    Opening {
        task: JoinHandle<()>,
        attempts: usize,
        /// Set when attaching again after a break: only this process will do.
        expect: Option<Binding>,
    },
    Live(Live),
    /// The stream broke. The same process is attached again after `at`, a bounded number of
    /// times.
    Retrying {
        binding: Option<Binding>,
        attempts: usize,
        at: Instant,
    },
    /// Not coming back, and the screen says why.
    Down,
}

pub(super) struct Live {
    pub(super) id: Vec<u8>,
    pub(super) binding: Binding,
    pub(super) to_remote: mpsc::Sender<ToRemote>,
    pub(super) pump: JoinHandle<()>,
    /// The size the attachment last heard of.
    pub(super) sent_size: (u16, u16),
    /// Re-attachments made since data last arrived.
    pub(super) attempts: usize,
    /// Whatever keeps the stream's tasks alive.
    pub(super) _guard: Option<Box<dyn Send>>,
    pub(super) retired: Option<oneshot::Receiver<()>>,
}

impl TileLink {
    pub(super) fn opening(
        model: ScreenModel,
        budget: RebuildBudget,
        generation: u64,
        task: JoinHandle<()>,
        attempts: usize,
        expect: Option<Binding>,
    ) -> Self {
        Self {
            model,
            budget,
            state: LinkState::Opening {
                task,
                attempts,
                expect,
            },
            generation,
        }
    }

    pub(super) fn live(&self) -> Option<&Live> {
        match &self.state {
            LinkState::Live(live) => Some(live),
            _ => None,
        }
    }

    /// Whether the surface may still become usable without the user doing anything.
    pub(super) fn may_recover(&self) -> bool {
        !matches!(self.state, LinkState::Down)
    }

    /// Stop everything for this surface: tell the far end, best effort, and stop reading. The
    /// receiver resolves when the far end has finished with the attachment.
    pub(super) fn close(self) -> Option<oneshot::Receiver<()>> {
        match self.state {
            LinkState::Opening { task, .. } => task.abort(),
            LinkState::Live(live) => {
                let _ = live.to_remote.try_send(ToRemote::Close);
                live.pump.abort();
                return live.retired;
            }
            LinkState::Retrying { .. } | LinkState::Down => {}
        }
        None
    }
}
