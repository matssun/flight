// SPDX-License-Identifier: MIT

use flight_state::HostId;

/// What the orchestrator knows about one node connection. The peer identity comes from the
/// authenticated transport, never from what the node says about itself.
#[derive(Debug, Clone)]
pub(crate) struct ConnState {
    pub(crate) peer: HostId,
    pub(crate) hello_done: bool,
}
