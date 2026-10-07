// SPDX-License-Identifier: MIT

use crate::{Fingerprint, TrustError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// What a node or UI remembers after joining: which orchestrator to dial, where, and under
/// what name. Public information only (the orchestrator's identity is pinned here); the
/// private key lives beside it in `identity/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub orchestrator: String,
    pub address: String,
    pub display_name: String,
}

impl ConnectionConfig {
    pub fn new(orchestrator: &Fingerprint, address: &str, display_name: &str) -> Self {
        Self {
            orchestrator: orchestrator.to_string(),
            address: address.to_owned(),
            display_name: display_name.to_owned(),
        }
    }

    /// The pinned orchestrator identity.
    pub fn orchestrator(&self) -> Result<Fingerprint, TrustError> {
        Fingerprint::parse(&self.orchestrator)
    }

    pub fn load(path: &Path) -> Result<Self, TrustError> {
        let text = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&text)
            .map_err(|e| TrustError::Invalid(format!("{}: {e}", path.display())))?;
        config.orchestrator()?;
        Ok(config)
    }

    /// Atomic: write a temp file, then rename.
    pub fn save(&self, path: &Path) -> Result<(), TrustError> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| TrustError::Invalid(format!("cannot write config: {e}")))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}
