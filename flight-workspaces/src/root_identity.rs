// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;

/// What the filesystem says a directory *is*, as opposed to what it is called. Recorded the
/// first time the root is seen, and compared when a missing path reappears: a restored backup,
/// a re-clone or a different volume at the same path has another identity, and Flight must not
/// assume it is the workspace the user saved. A mismatch is reported, never acted on or
/// rejected; only the user can accept it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootIdentity {
    pub dev: u64,
    pub ino: u64,
}

impl RootIdentity {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }
}
