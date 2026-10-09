// SPDX-License-Identifier: MIT

/// What a user asked of a saved workspace, detached from the wire. None of these creates,
/// repairs or deletes anything on the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedAction {
    /// Look again now.
    Retry,
    /// Forget the saved reference; running processes and files are untouched.
    Remove,
    /// Start the workspace again (a replacement process) in its verified root.
    Restore,
    /// Start the workspace as a new conversation, on purpose: the earlier one is not continued
    /// (and is not presented as continued), and the new agent's session replaces its reference.
    RestoreFresh,
    /// The directory now at the path is the one meant: remember it.
    AcceptRoot,
    /// Point the saved workspace at another directory. Nothing is created there.
    SetRoot(String),
    /// An imported definition is trusted and may start processes.
    Trust,
}

/// A validated operation on one of this node's saved workspaces, named by its stable key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedActionRequest {
    pub config_key: String,
    pub action: SavedAction,
}
