// SPDX-License-Identifier: MIT

use super::Program;
use flight_state::HostId;

/// A validated request to create a session: where, what it is called, where it starts and
/// what it runs. The backend carries it to the node; the dashboard never runs anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSessionRequest {
    pub host: HostId,
    /// The host's display name, for messages.
    pub host_label: String,
    pub name: String,
    pub dir: String,
    pub program: Program,
}
