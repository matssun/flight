// SPDX-License-Identifier: MIT

use crate::{Reject, Validate};

#[derive(Clone, Copy, PartialEq, Eq, prost::Message)]
pub struct Heartbeat {
    #[prost(uint64, tag = "1")]
    pub seq: u64,
}

impl Validate for Heartbeat {
    fn validate(&self) -> Result<(), Reject> {
        Ok(())
    }
}
