// SPDX-License-Identifier: MIT

/// Why a session was not created, as the dashboard can tell it apart. Nothing was created in
/// any of these cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateFailure {
    /// The node already has a session with that name.
    AlreadyExists,
    /// The node says the directory is missing or not a directory.
    NoSuchDirectory(String),
    /// The node could not start the program (not installed, or it ended at once).
    ProgramUnavailable(String),
    /// The node is not connected.
    Unreachable,
    /// The workspace is not (or no longer) known to the node or the orchestrator.
    UnknownWorkspace,
    /// This dashboard has no way to create sessions (it is not reading an orchestrator).
    Unsupported,
    /// Anything else, in the words of whoever refused.
    Other(String),
}
