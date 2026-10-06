// SPDX-License-Identifier: MIT

use crate::Reject;

/// The protocol version this build speaks. A major bump is a breaking change; a minor bump
/// only adds optional fields, messages or capabilities.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, PartialEq, Eq, prost::Message)]
pub struct ProtocolVersion {
    #[prost(uint32, tag = "1")]
    pub major: u32,
    #[prost(uint32, tag = "2")]
    pub minor: u32,
}

impl ProtocolVersion {
    /// The version both sides will speak: same major required, the lower minor wins.
    pub fn negotiate(self, peer: ProtocolVersion) -> Result<ProtocolVersion, Reject> {
        if self.major != peer.major {
            return Err(Reject::MajorMismatch {
                ours: self.major,
                theirs: peer.major,
            });
        }
        Ok(ProtocolVersion {
            major: self.major,
            minor: self.minor.min(peer.minor),
        })
    }
}
