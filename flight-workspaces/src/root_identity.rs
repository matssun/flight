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
    /// When the directory was created, in nanoseconds since the epoch, where the filesystem
    /// records it. Inode numbers are reused: on Linux a directory deleted and made again is
    /// often given the same one, so device and inode alone cannot tell the two apart. Absent
    /// where the filesystem has no creation time; then that case cannot be detected, which
    /// is why a verified root is "not contradicted", not "proven".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth_ns: Option<i64>,
}

impl RootIdentity {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        let birth_ns = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_nanos()).ok());
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            birth_ns,
        }
    }

    /// Whether `other` can be the same directory: the same device and inode, and the same
    /// creation time when both sides know one.
    pub fn same_directory(&self, other: &Self) -> bool {
        self.dev == other.dev
            && self.ino == other.ino
            && match (self.birth_ns, other.birth_ns) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}
