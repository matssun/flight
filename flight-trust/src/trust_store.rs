// SPDX-License-Identifier: MIT

use crate::{Fingerprint, TrustError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Node,
    Ui,
}

/// This orchestrator's own identity, so a copy of the file pins it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorEntry {
    pub fingerprint: String,
    pub display_name: String,
}

fn enabled_by_default() -> bool {
    true
}

/// One authorized identity. The id *is* the fingerprint; it is not stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub id: String,
    pub display_name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TrustFile {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orchestrator: Option<OrchestratorEntry>,
    #[serde(default)]
    nodes: Vec<TrustedPeer>,
}

/// The explicit allowlist of identities this orchestrator currently authorizes. Public trust
/// decisions only: no keys, no tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustStore {
    orchestrator: Option<OrchestratorEntry>,
    peers: Vec<TrustedPeer>,
}

impl TrustStore {
    pub fn empty() -> Self {
        Self {
            orchestrator: None,
            peers: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, TrustError> {
        let file: TrustFile =
            toml::from_str(text).map_err(|e| TrustError::Invalid(format!("trust file: {e}")))?;
        if file.version != VERSION {
            return Err(TrustError::Invalid(format!(
                "unsupported trust file version {}",
                file.version
            )));
        }
        if let Some(o) = &file.orchestrator {
            Fingerprint::parse(&o.fingerprint)?;
        }
        let mut seen = std::collections::HashSet::new();
        for peer in &file.nodes {
            Fingerprint::parse(&peer.id)?;
            if !seen.insert(peer.id.clone()) {
                return Err(TrustError::Invalid(format!(
                    "duplicate trusted id {}",
                    peer.id
                )));
            }
        }
        Ok(Self {
            orchestrator: file.orchestrator,
            peers: file.nodes,
        })
    }

    pub fn to_toml(&self) -> Result<String, TrustError> {
        toml::to_string_pretty(&TrustFile {
            version: VERSION,
            orchestrator: self.orchestrator.clone(),
            nodes: self.peers.clone(),
        })
        .map_err(|e| TrustError::Invalid(format!("cannot write trust file: {e}")))
    }

    /// A missing file is an empty store: nothing is trusted yet.
    pub fn load(path: &Path) -> Result<Self, TrustError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(e.into()),
        }
    }

    /// Write atomically (temp file, then rename) so a crash never leaves a torn file.
    pub fn save(&self, path: &Path) -> Result<(), TrustError> {
        let tmp = path.with_extension("toml.tmp");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&tmp, self.to_toml()?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn orchestrator(&self) -> Option<&OrchestratorEntry> {
        self.orchestrator.as_ref()
    }

    pub fn set_orchestrator(&mut self, fingerprint: &Fingerprint, display_name: &str) {
        self.orchestrator = Some(OrchestratorEntry {
            fingerprint: fingerprint.to_string(),
            display_name: display_name.to_owned(),
        });
    }

    pub fn peers(&self) -> &[TrustedPeer] {
        &self.peers
    }

    /// Whether `id` is currently authorized in `role`. Unknown and disabled are both `false`.
    pub fn is_authorized(&self, id: &Fingerprint, role: Role) -> bool {
        self.peers
            .iter()
            .any(|p| p.id == id.as_str() && p.enabled && p.role == role)
    }

    /// Authorize (or re-enable and rename) an identity.
    pub fn authorize(&mut self, id: &Fingerprint, display_name: &str, role: Role) {
        match self.peers.iter_mut().find(|p| p.id == id.as_str()) {
            Some(p) => {
                p.display_name = display_name.to_owned();
                p.enabled = true;
                p.role = role;
            }
            None => self.peers.push(TrustedPeer {
                id: id.to_string(),
                display_name: display_name.to_owned(),
                enabled: true,
                role,
            }),
        }
    }

    /// Revoke without forgetting. Returns whether the identity was known.
    pub fn disable(&mut self, id: &Fingerprint) -> bool {
        self.peers
            .iter_mut()
            .find(|p| p.id == id.as_str())
            .map(|p| p.enabled = false)
            .is_some()
    }

    pub fn remove(&mut self, id: &Fingerprint) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| p.id != id.as_str());
        self.peers.len() != before
    }
}
