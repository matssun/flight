// SPDX-License-Identifier: MIT

use crate::{HostId, ServerId, SessionId};

/// Globally unambiguous reference to a session. The session *name* is metadata and can
/// collide across hosts or change over time; this cannot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionRef {
    pub host: HostId,
    pub server: ServerId,
    pub session: SessionId,
}
