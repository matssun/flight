// SPDX-License-Identifier: MIT

use std::fmt;

/// Why a transport operation failed.
#[derive(Debug)]
pub enum TransportError {
    Io(std::io::Error),
    Trust(flight_trust::TrustError),
    /// Connecting or the TLS handshake failed (including a server whose fingerprint is not
    /// the pinned one).
    Connect(String),
    /// The operating system answered "no route to host / network unreachable" before sending
    /// anything. Distinct from a peer that is down (refused, timed out): see `NodeLink`.
    Unreachable(String),
    /// The peer refused the request (not authorized, bad token, ...).
    Refused(String),
    Protocol(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "i/o error: {e}"),
            Self::Trust(e) => write!(f, "{e}"),
            Self::Connect(m) => write!(f, "cannot connect: {m}"),
            Self::Unreachable(m) => write!(f, "cannot connect (no route): {m}"),
            Self::Refused(m) => write!(f, "refused: {m}"),
            Self::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl TransportError {
    /// Whether this machine itself could not route to the peer (as opposed to the peer
    /// being down or refusing).
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable(_))
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<flight_trust::TrustError> for TransportError {
    fn from(e: flight_trust::TrustError) -> Self {
        Self::Trust(e)
    }
}

impl From<tonic::Status> for TransportError {
    fn from(s: tonic::Status) -> Self {
        match s.code() {
            tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => {
                Self::Refused(s.message().to_owned())
            }
            _ => Self::Protocol(s.to_string()),
        }
    }
}
