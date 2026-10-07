// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{Reject, Validate};

wire_enum! {
    /// Whether a node can observe one of its tmux servers.
    AvailabilityCode { Available = 1, TmuxUnavailable = 2, NoServer = 3, Failed = 4 }
}

#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct ServerStatus {
    #[prost(string, tag = "1")]
    pub server: String,
    #[prost(enumeration = "AvailabilityCode", tag = "2")]
    pub availability: i32,
    #[prost(string, tag = "3")]
    pub detail: String,
}

impl Validate for ServerStatus {
    fn validate(&self) -> Result<(), Reject> {
        non_empty(&self.server, "server_status.server")?;
        AvailabilityCode::decode(self.availability, "server_status.availability").map(|_| ())
    }
}
