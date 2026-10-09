// SPDX-License-Identifier: MIT

/// Why a saved document could not be used. In every case the file on disk is left as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Not valid TOML or not a valid document.
    Corrupt(String),
    /// Written by a newer Flight. Read-only to this build.
    TooNew {
        found: u32,
    },
    Migration(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(why) => write!(f, "the saved workspaces cannot be read: {why}"),
            Self::TooNew { found } => write!(
                f,
                "the saved workspaces use schema {found}, newer than this Flight; not touching them"
            ),
            Self::Migration(why) => write!(f, "upgrading the saved workspaces failed: {why}"),
        }
    }
}

impl std::error::Error for LoadError {}
