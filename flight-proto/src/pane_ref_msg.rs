// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{Reject, Validate};
use flight_state::{HostId, PaneId, PaneRef, ServerId};

/// A pane's identity: host (the node), tmux server and pane id.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct PaneRefMsg {
    #[prost(string, tag = "1")]
    pub host: String,
    #[prost(string, tag = "2")]
    pub server: String,
    #[prost(string, tag = "3")]
    pub pane: String,
}

impl Validate for PaneRefMsg {
    fn validate(&self) -> Result<(), Reject> {
        non_empty(&self.host, "pane_ref.host")?;
        non_empty(&self.server, "pane_ref.server")?;
        non_empty(&self.pane, "pane_ref.pane")
    }
}

impl From<&PaneRef> for PaneRefMsg {
    fn from(r: &PaneRef) -> Self {
        Self {
            host: r.host.as_str().to_owned(),
            server: r.server.as_str().to_owned(),
            pane: r.pane.as_str().to_owned(),
        }
    }
}

impl TryFrom<&PaneRefMsg> for PaneRef {
    type Error = Reject;

    fn try_from(m: &PaneRefMsg) -> Result<Self, Reject> {
        m.validate()?;
        Ok(PaneRef {
            host: HostId::new(m.host.as_str()),
            server: ServerId::new(m.server.as_str()),
            pane: PaneId::new(m.pane.as_str()),
        })
    }
}
