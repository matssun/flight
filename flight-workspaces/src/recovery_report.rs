// SPDX-License-Identifier: MIT

use crate::plan::{Action, Item};
use crate::{ConfigKey, ExecError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Bound,
    Started,
    Resumed,
    Refused(String),
    /// Outcome unknown; left for the next pass to observe.
    Unknown(String),
}

/// What a recovery pass saw and did. `changed` says whether the document needs saving.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// The state of every saved workspace after planning.
    pub items: Vec<Item>,
    pub done: Vec<(ConfigKey, Action, Outcome)>,
    /// Running workspaces recorded into the saved set in this pass.
    pub recorded: Vec<String>,
    pub changed: bool,
}

impl RecoveryReport {
    pub(crate) fn refused(e: ExecError) -> Outcome {
        match e {
            ExecError::Refused(m) => Outcome::Refused(m),
            ExecError::Unknown(m) => Outcome::Unknown(m),
        }
    }
}
