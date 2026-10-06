// SPDX-License-Identifier: MIT

use super::{EventKind, NotificationType};

/// One parsed line of an agent's event log. Parsing the log is the caller's job; this
/// crate only interprets entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEntry {
    pub kind: EventKind,
    /// Seconds since the epoch.
    pub ts: u64,
    pub tool: Option<String>,
    pub stop_reason: Option<String>,
    pub background_tasks: bool,
    pub notification_type: Option<NotificationType>,
}

impl EventEntry {
    pub fn new(kind: EventKind, ts: u64) -> Self {
        Self {
            kind,
            ts,
            tool: None,
            stop_reason: None,
            background_tasks: false,
            notification_type: None,
        }
    }
}
