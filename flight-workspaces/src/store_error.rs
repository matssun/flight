// SPDX-License-Identifier: MIT

use crate::LoadError;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Load(LoadError),
    /// The file on disk was written by a newer Flight; this build will not overwrite it.
    NewerOnDisk {
        found: u32,
    },
    /// A snapshot label that is not plain text.
    BadLabel,
    /// A snapshot with this label exists; snapshots are never overwritten.
    Exists,
    NotFound,
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<LoadError> for StoreError {
    fn from(e: LoadError) -> Self {
        Self::Load(e)
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Load(e) => write!(f, "{e}"),
            Self::NewerOnDisk { found } => {
                write!(
                    f,
                    "saved workspaces are schema {found}; refusing to overwrite"
                )
            }
            Self::BadLabel => f.write_str("a snapshot label is letters, digits, '_', '-' and '.'"),
            Self::Exists => f.write_str("a snapshot with that label already exists"),
            Self::NotFound => f.write_str("no such snapshot"),
        }
    }
}

impl std::error::Error for StoreError {}
