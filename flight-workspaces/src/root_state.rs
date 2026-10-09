// SPDX-License-Identifier: MIT

use crate::{GitMarker, RootIdentity};

/// What looking at a root found. The failure cases are kept apart because they call for
/// different things from the user: a path that is gone, a disk that is not mounted, a
/// permission to fix and a host to bring back are not the same problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootState {
    Present {
        identity: RootIdentity,
        git: GitMarker,
    },
    /// The path does not exist, and its parent does and is readable: confirmed absent.
    Missing,
    NotADirectory,
    PermissionDenied,
    /// Could not be confirmed either way: an ancestor is missing or looks like an unmounted
    /// mount point, or the filesystem returned an error that says nothing about the path.
    Unverified {
        reason: String,
    },
    /// The host that owns the path could not be asked.
    HostUnreachable {
        reason: String,
    },
}
