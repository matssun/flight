// SPDX-License-Identifier: MIT

use crate::{Fingerprint, TrustError};
use std::fmt;

/// What an operator copies to a new node: `orchestrator=<fingerprint> address=<host:port>
/// token=<secret> expires=<epoch secs>`. Packaged together for convenience; the protocol
/// treats the pinned identity (who to trust) and the token (permission to join) separately.
#[derive(Clone, PartialEq, Eq)]
pub struct EnrollmentBundle {
    pub orchestrator: Fingerprint,
    pub address: String,
    pub token: String,
    pub expires_at: u64,
}

impl fmt::Display for EnrollmentBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "orchestrator={} address={} token={} expires={}",
            self.orchestrator, self.address, self.token, self.expires_at
        )
    }
}

impl fmt::Debug for EnrollmentBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnrollmentBundle")
            .field("orchestrator", &self.orchestrator)
            .field("address", &self.address)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl EnrollmentBundle {
    pub fn parse(text: &str) -> Result<Self, TrustError> {
        let (mut orchestrator, mut address, mut token, mut expires) = (None, None, None, None);
        for part in text.split_whitespace() {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| TrustError::Invalid(format!("expected key=value, got {part:?}")))?;
            let slot = match key {
                "orchestrator" => &mut orchestrator,
                "address" => &mut address,
                "token" => &mut token,
                "expires" => &mut expires,
                other => return Err(TrustError::Invalid(format!("unknown field {other:?}"))),
            };
            if slot.replace(value.to_owned()).is_some() {
                return Err(TrustError::Invalid(format!("duplicate field {key:?}")));
            }
        }
        let missing = |name: &str| TrustError::Invalid(format!("missing {name}"));
        Ok(Self {
            orchestrator: Fingerprint::parse(
                &orchestrator.ok_or_else(|| missing("orchestrator"))?,
            )?,
            address: address.ok_or_else(|| missing("address"))?,
            token: token.ok_or_else(|| missing("token"))?,
            expires_at: expires
                .ok_or_else(|| missing("expires"))?
                .parse()
                .map_err(|_| TrustError::Invalid("expires must be a number".to_owned()))?,
        })
    }
}
