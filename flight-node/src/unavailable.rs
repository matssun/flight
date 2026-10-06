// SPDX-License-Identifier: MIT

use flight_proto::{AvailabilityCode, ServerStatus};
use flight_state::ServerId;

/// Why a tmux server could not be observed this round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// tmux is not installed.
    TmuxMissing,
    /// tmux is installed but no server runs on the endpoint: its agents are gone.
    NoServer,
    /// tmux failed in some other way. The server may well still be running, so its panes
    /// are kept (stale) rather than reported gone.
    Failed(String),
}

impl Unavailable {
    /// Whether the server's panes (and their Tracking) no longer exist.
    pub(crate) fn panes_are_gone(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }

    pub(crate) fn status(&self, server: &ServerId) -> ServerStatus {
        let (availability, detail) = match self {
            Self::TmuxMissing => (AvailabilityCode::TmuxUnavailable, String::new()),
            Self::NoServer => (AvailabilityCode::NoServer, String::new()),
            Self::Failed(d) => (AvailabilityCode::Failed, d.clone()),
        };
        ServerStatus {
            server: server.as_str().to_owned(),
            availability: availability as i32,
            detail,
        }
    }
}

pub(crate) fn available(server: &ServerId) -> ServerStatus {
    ServerStatus {
        server: server.as_str().to_owned(),
        availability: AvailabilityCode::Available as i32,
        detail: String::new(),
    }
}
