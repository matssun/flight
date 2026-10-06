// SPDX-License-Identifier: MIT

use crate::TrustError;
use flight_state::HostId;
use sha2::{Digest, Sha256};
use std::fmt;

const PREFIX: &str = "sha256:";

/// `sha256:<64 hex>` of a certificate's SubjectPublicKeyInfo. It is the identity: a node's
/// `NodeId`/`HostId` and the orchestrator's pinned identity. Stable across certificate
/// reissue as long as the key is the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub(crate) fn of_spki(spki_der: &[u8]) -> Self {
        let digest = Sha256::digest(spki_der);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        Self(format!("{PREFIX}{hex}"))
    }

    pub fn parse(text: &str) -> Result<Self, TrustError> {
        let hex = text
            .strip_prefix(PREFIX)
            .ok_or_else(|| TrustError::Invalid(format!("fingerprint must start with {PREFIX}")))?;
        let ok = hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !ok {
            return Err(TrustError::Invalid(
                "fingerprint must be 64 lowercase hex digits".to_owned(),
            ));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `HostId` (= `NodeId`) this fingerprint denotes.
    pub fn host_id(&self) -> HostId {
        HostId::new(self.0.as_str())
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
