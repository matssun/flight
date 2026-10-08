// SPDX-License-Identifier: MIT

use flight_state::HostId;

/// A node the form offers: a host the dashboard currently has a live link to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostChoice {
    pub host: HostId,
    pub label: String,
}
