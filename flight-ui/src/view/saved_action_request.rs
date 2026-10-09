// SPDX-License-Identifier: MIT

use flight_state::HostId;

/// What a user can do to a saved workspace. None of these creates, repairs or deletes anything
/// on the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedActionKind {
    /// Look again now.
    Retry,
    /// Forget the saved reference. Running processes, directories and repositories stay.
    Remove,
    /// Start it again as a replacement process, in its verified directory.
    Restore,
    /// The directory now at the path is the one meant.
    AcceptRoot,
    /// Point it at another directory (nothing is created there).
    SetRoot(String),
    /// An imported workspace is trusted and may start processes.
    Trust,
}

/// A request to act on one saved workspace, on the host that saved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedActionRequest {
    pub host: HostId,
    pub host_label: String,
    /// The stable saved identity.
    pub config_key: String,
    pub name: String,
    pub action: SavedActionKind,
}
