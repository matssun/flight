// SPDX-License-Identifier: MIT

use crate::{ConfigKey, SurfaceKind, WorkspaceDefinition};

/// A started workspace, as the backend reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub workspace_id: String,
}

/// Why a start did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// Nothing was started and nothing will be: safe to report and leave.
    Refused(String),
    /// The request may or may not have been applied (a timeout, a lost reply). It is never
    /// retried in the same pass; the next pass observes before it acts.
    Unknown(String),
}

/// Starts processes. An implementation must write the definition's [`ConfigKey`] (and each
/// surface's) into the backend in the same step that creates the workspace or surface, so that
/// a reply lost on the way back still leaves a mark the next observation finds. It must not
/// create, move or remove anything on the filesystem.
pub trait Executor {
    fn start_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError>;

    /// Start the workspace with its agent continuing the earlier session. Must verify that the
    /// session can be continued *before* starting anything, and must not fall back to a
    /// replacement: a replacement is a different agent and is started only by `start_workspace`.
    fn resume_workspace(&mut self, def: &WorkspaceDefinition) -> Result<Started, ExecError>;

    fn start_surface(
        &mut self,
        def: &WorkspaceDefinition,
        surface: &ConfigKey,
        kind: SurfaceKind,
        workspace_id: &str,
    ) -> Result<(), ExecError>;

    fn resume_agent(
        &mut self,
        def: &WorkspaceDefinition,
        surface: &ConfigKey,
        workspace_id: &str,
    ) -> Result<(), ExecError>;
}
