// SPDX-License-Identifier: MIT

use crate::TrustError;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

/// Default lifetime of an enrollment token.
pub const DEFAULT_TTL_SECS: u64 = 600;
const TOKEN_BYTES: usize = 32;

/// A freshly issued token: shown to the operator once, never stored in the clear.
#[derive(Clone, PartialEq, Eq)]
pub struct IssuedToken {
    pub secret: String,
    /// Seconds since the epoch.
    pub expires_at: u64,
}

impl fmt::Debug for IssuedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedToken")
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Why a token was refused. Distinct for logs and tests; callers must not expose the
/// difference to the peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollError {
    /// Never issued, or already used.
    Invalid,
    Expired,
}

/// Outstanding enrollment tokens: single use, short lived, held only as SHA-256 hashes, in
/// memory. An orchestrator restart discards them.
#[derive(Default)]
pub struct EnrollmentTokens {
    expiry_by_hash: HashMap<[u8; 32], u64>,
}

fn hash(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

impl EnrollmentTokens {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(&mut self, now: u64, ttl_secs: u64) -> Result<IssuedToken, TrustError> {
        self.expiry_by_hash.retain(|_, exp| *exp > now);
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::getrandom(&mut bytes).map_err(|e| TrustError::Generate(e.to_string()))?;
        let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let expires_at = now.saturating_add(ttl_secs);
        self.expiry_by_hash.insert(hash(&secret), expires_at);
        Ok(IssuedToken { secret, expires_at })
    }

    /// Consume a token. Whatever the outcome, the token cannot be used again.
    pub fn redeem(&mut self, secret: &str, now: u64) -> Result<(), EnrollError> {
        match self.expiry_by_hash.remove(&hash(secret)) {
            None => Err(EnrollError::Invalid),
            Some(expires_at) if expires_at <= now => Err(EnrollError::Expired),
            Some(_) => Ok(()),
        }
    }

    pub fn outstanding(&self) -> usize {
        self.expiry_by_hash.len()
    }
}
