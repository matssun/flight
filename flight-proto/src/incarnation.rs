// SPDX-License-Identifier: MIT

use crate::Reject;

/// Identifies one incarnation of a stream's producer (a node process, or an orchestrator
/// run). It is random, never a counter and never persisted: a restarted producer has a new
/// one, so a receiver holding old messages cannot mistake a restart for a continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Incarnation([u8; Incarnation::LEN]);

impl Incarnation {
    pub const LEN: usize = 16;

    pub fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, Reject> {
        <[u8; Self::LEN]>::try_from(bytes)
            .map(Self)
            .map_err(|_| Reject::OutOfRange("incarnation"))
    }
}
