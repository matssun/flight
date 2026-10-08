// SPDX-License-Identifier: MIT

//! The orchestrator's end of a terminal: two bounded queues and a way to abort. Terminal bytes
//! are copied between the two streams and never interpreted.

use flight_orchestrator::Side;
use flight_proto::{ExitReasonCode, TerminalFrame};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

/// Frames queued per direction. With 16 KiB at most per data frame, 64 KiB per direction.
pub(crate) const QUEUE_FRAMES: usize = 4;

/// What one side of a terminal stream needs: where its inbound frames go, where its outbound
/// frames come from, and the abort signal.
pub(crate) struct Ends {
    /// Toward the other side.
    pub(crate) forward: mpsc::Sender<TerminalFrame>,
    /// From the other side; becomes this side's response stream.
    pub(crate) incoming: mpsc::Receiver<TerminalFrame>,
    pub(crate) abort: watch::Receiver<Option<ExitReasonCode>>,
    pub(crate) peak: Arc<AtomicUsize>,
}

/// Both queues of one terminal, until each side has claimed its half.
pub(crate) struct Relay {
    to_node: Option<mpsc::Sender<TerminalFrame>>,
    to_ui: Option<mpsc::Sender<TerminalFrame>>,
    for_node: Option<mpsc::Receiver<TerminalFrame>>,
    for_ui: Option<mpsc::Receiver<TerminalFrame>>,
    abort: watch::Sender<Option<ExitReasonCode>>,
    peak: Arc<AtomicUsize>,
    pub(crate) started: std::time::Instant,
}

impl Relay {
    pub(crate) fn new() -> Self {
        let (to_node, for_node) = mpsc::channel(QUEUE_FRAMES);
        let (to_ui, for_ui) = mpsc::channel(QUEUE_FRAMES);
        let (abort, _) = watch::channel(None);
        Self {
            to_node: Some(to_node),
            to_ui: Some(to_ui),
            for_node: Some(for_node),
            for_ui: Some(for_ui),
            abort,
            peak: Arc::new(AtomicUsize::new(0)),
            started: std::time::Instant::now(),
        }
    }

    /// Hand a side its half. A half can be claimed once (the core already refuses a second
    /// attach; this is the same rule one level down).
    pub(crate) fn claim(&mut self, side: Side) -> Option<Ends> {
        let (forward, incoming) = match side {
            Side::Node => (self.to_ui.take()?, self.for_node.take()?),
            Side::Ui => (self.to_node.take()?, self.for_ui.take()?),
        };
        Some(Ends {
            forward,
            incoming,
            abort: self.abort.subscribe(),
            peak: self.peak.clone(),
        })
    }

    /// Tell whichever sides are attached that the terminal is over, and why.
    pub(crate) fn abort(&self, reason: ExitReasonCode) {
        self.abort.send_replace(Some(reason));
    }

    pub(crate) fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }
}
