// SPDX-License-Identifier: MIT

use crate::{ConfigKey, RootSpec, SurfaceSpec};
use serde::{Deserialize, Serialize};

/// Where a definition came from. Imported definitions never start a process on their own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    #[default]
    Local,
    Imported,
}

/// The user's declared intent for one workspace. Observed state (is it running, what are its
/// ids) is never written into the declaration, except as clearly named hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDefinition {
    pub key: ConfigKey,
    pub name: String,
    /// The host that owns the root. A label the registry resolves; no address or credential.
    pub host: String,
    pub root: RootSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<SurfaceSpec>,
    #[serde(default)]
    pub origin: Origin,
    /// The runtime `WorkspaceId` last seen. A hint for matching, never an authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_workspace_id: Option<String>,
}
