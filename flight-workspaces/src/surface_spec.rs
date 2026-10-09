// SPDX-License-Identifier: MIT

use crate::ConfigKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceKind {
    Agent,
    Shell,
}

/// A surface the workspace should have. No command line, environment or credential is stored
/// here: a definition can be shared, and what it may start is a separate trust decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceSpec {
    pub key: ConfigKey,
    pub kind: SurfaceKind,
    /// The agent provider (`claude`), for an Agent surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The agent runs without asking before acting. Starting such an agent from a saved
    /// definition needs its own permission (see `RecoveryPolicy`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_permissions: bool,
    /// The runtime `SurfaceId` last seen. A hint for matching, never an authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_surface_id: Option<String>,
}
