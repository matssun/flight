// SPDX-License-Identifier: MIT

use crate::HostError;
use flight_state::{HostId, PaneRef, ServerId};
use flight_tmux::PaneInfo;

/// A pane with its globally unambiguous identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPane {
    pub pane_ref: PaneRef,
    pub info: PaneInfo,
}

/// The pane listing of one (host, server). One unreachable host must not hide the others,
/// so listing everything yields one outcome per endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanesOutcome {
    pub host: HostId,
    pub server: ServerId,
    pub result: Result<Vec<HostPane>, HostError>,
}
