// SPDX-License-Identifier: MIT

use crate::session::{Attachment, Binding};
use flight_state::SurfaceId;
use flight_ui::SurfaceChoice;
use std::future::Future;

/// Why a surface could not be attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenFailure {
    /// The surface is gone, changed, or this identity may not open it. Trying again cannot help.
    Refused(String),
    /// The link or the node was not reachable. Trying again later may work.
    Unavailable(String),
}

/// A request to attach a surface of the workspace being worked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub choice: SurfaceChoice,
    /// Which surface of that kind, when the workspace has more than one: its own id. `None`:
    /// the workspace's agent or shell, found by kind.
    pub surface: Option<SurfaceId>,
    pub cols: u16,
    pub rows: u16,
    /// Set when re-attaching after a lost stream: only this exact process may be attached.
    pub expect: Option<Binding>,
}

/// What the session needs from the process around it: the link that is already up, and nothing
/// about how the terminal is carried. The real host resolves the workspace's surface, asks the
/// orchestrator for a terminal and connects the stream; tests use one that is in memory.
pub trait SurfaceHost: Send + Sync + 'static {
    /// Attach a surface: resolve it, ask for a terminal and connect to it.
    fn open(
        &self,
        request: OpenRequest,
    ) -> impl Future<Output = Result<Attachment, OpenFailure>> + Send;

    /// Connect to a terminal that was already requested (the dashboard asked for it).
    fn connect(
        &self,
        id: Vec<u8>,
        choice: SurfaceChoice,
        binding: Binding,
    ) -> impl Future<Output = Result<Attachment, OpenFailure>> + Send;

    /// Say the presentation of this terminal is alive (ADR-004). An error means the link is
    /// gone.
    fn renew(&self, attachment: &[u8]) -> Result<(), String>;
}
