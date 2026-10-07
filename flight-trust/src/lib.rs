// SPDX-License-Identifier: MIT

//! Identity, trust and enrollment for Flight (ADR-002).
//!
//! - [`Identity`]: a keypair with a self-signed certificate; its [`Fingerprint`] is the id.
//! - [`TrustStore`]: the operator's allowlist (`trust.toml`), public decisions only.
//! - [`EnrollmentTokens`]: single-use, short-lived, hashed, in-memory join secrets.
//! - [`client_config`] / [`server_config`]: mutual TLS 1.3 verified by fingerprint, never by
//!   CA or hostname.

mod bundle;
mod cert;
mod connection_config;
mod enrollment;
mod error;
mod fingerprint;
mod identity;
mod tls;
mod trust_store;

pub use bundle::EnrollmentBundle;
pub use cert::fingerprint_of_cert;
pub use connection_config::ConnectionConfig;
pub use enrollment::{EnrollError, EnrollmentTokens, IssuedToken, DEFAULT_TTL_SECS};
pub use error::TrustError;
pub use fingerprint::Fingerprint;
pub use identity::Identity;
pub use tls::{client_config, peer_fingerprint, server_config};
pub use trust_store::{OrchestratorEntry, Role, TrustStore, TrustedPeer};
