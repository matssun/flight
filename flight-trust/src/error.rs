// SPDX-License-Identifier: MIT

use std::fmt;

/// Why an identity, trust or enrollment operation failed.
#[derive(Debug)]
pub enum TrustError {
    Io(std::io::Error),
    /// A fingerprint, certificate, key or trust file that does not parse.
    Invalid(String),
    /// Generating an identity failed.
    Generate(String),
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "i/o error: {e}"),
            Self::Invalid(m) => write!(f, "invalid: {m}"),
            Self::Generate(m) => write!(f, "cannot generate identity: {m}"),
        }
    }
}

impl std::error::Error for TrustError {}

impl From<std::io::Error> for TrustError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
