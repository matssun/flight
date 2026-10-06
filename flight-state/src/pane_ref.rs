// SPDX-License-Identifier: MIT

use crate::{HostId, PaneId, ServerId};

/// Globally unambiguous reference to a pane. Two hosts may both have pane `%1`; only the
/// full reference is identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneRef {
    pub host: HostId,
    pub server: ServerId,
    pub pane: PaneId,
}
