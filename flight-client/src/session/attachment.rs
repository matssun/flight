// SPDX-License-Identifier: MIT

use flight_proto::ExitReasonCode;
use flight_state::PaneRef;
use tokio::sync::{mpsc, oneshot};

/// What the user sees is bound to one process in one pane. A reconnect must find the same one:
/// input typed for an agent is never delivered to the agent that replaced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub pane: PaneRef,
    pub pid: u32,
}

/// What the session sends to an attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToRemote {
    Data(Vec<u8>),
    Resize(u16, u16),
    /// The user is done with this attachment. Sent best effort before it is dropped.
    Close,
}

/// What an attachment reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromRemote {
    Data(Vec<u8>),
    /// The node ended the terminal and said why.
    Exit {
        reason: ExitReasonCode,
        status: i32,
    },
    /// The stream broke without a report.
    Lost(String),
}

/// One terminal stream to one surface. Both channels are bounded, so a stalled far end slows
/// the session instead of growing memory. Dropping it ends the stream.
pub struct Attachment {
    pub id: Vec<u8>,
    pub binding: Binding,
    pub to_remote: mpsc::Sender<ToRemote>,
    pub from_remote: mpsc::Receiver<FromRemote>,
    /// Whatever keeps the stream's tasks alive; dropped with the attachment.
    pub guard: Option<Box<dyn Send>>,
    /// Resolves when the far end has finished with this attachment after it was let go: what
    /// was sent has been read and the stream is over. `None` when there is nothing to wait for.
    /// A new attachment to the same surface must not deliver input before this, or its keys
    /// could overtake keys still in flight on this one.
    pub retired: Option<oneshot::Receiver<()>>,
}
