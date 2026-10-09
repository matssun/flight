// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{Reject, SavedHealthCode, SavedResumeCode, SavedRootCode, Validate};

/// The longest failure detail the wire carries.
pub const MAX_DETAIL_LEN: usize = 512;
const MAX_NAME_LEN: usize = 64;

/// One workspace a node has saved, with how it stands now (ADR-008): declared intent plus the
/// node's own reading of it. The node is the only party that can verify the root.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct SavedWorkspace {
    /// The stable saved identity; not a runtime workspace id.
    #[prost(string, tag = "1")]
    pub config_key: String,
    #[prost(string, tag = "2")]
    pub name: String,
    /// The configured root directory on the node.
    #[prost(string, tag = "3")]
    pub root: String,
    #[prost(enumeration = "SavedHealthCode", tag = "4")]
    pub health: i32,
    #[prost(enumeration = "SavedRootCode", tag = "5")]
    pub root_state: i32,
    /// Failure information, plain text.
    #[prost(string, tag = "6")]
    pub detail: String,
    /// The running workspace that realizes it, when one does.
    #[prost(string, tag = "7")]
    pub workspace_id: String,
    /// Imported rather than made on this node: starts nothing on its own.
    #[prost(bool, tag = "8")]
    pub imported: bool,
    /// What starting it again does about the agent's conversation (ADR-010). 0 from a node that
    /// does not say.
    #[prost(enumeration = "SavedResumeCode", tag = "9")]
    pub resume: i32,
    /// Why a conversation cannot be continued, plain text.
    #[prost(string, tag = "10")]
    pub resume_detail: String,
}

impl Validate for SavedWorkspace {
    fn validate(&self) -> Result<(), Reject> {
        if !flight_state::valid_id(&self.config_key) {
            return Err(Reject::OutOfRange("saved_workspace.config_key"));
        }
        non_empty(&self.name, "saved_workspace.name")?;
        if self.name.len() > MAX_NAME_LEN {
            return Err(Reject::OutOfRange("saved_workspace.name"));
        }
        if self.root.len() > flight_state::MAX_DIR_LEN {
            return Err(Reject::OutOfRange("saved_workspace.root"));
        }
        if self.detail.len() > MAX_DETAIL_LEN {
            return Err(Reject::OutOfRange("saved_workspace.detail"));
        }
        if !self.workspace_id.is_empty() && !flight_state::valid_id(&self.workspace_id) {
            return Err(Reject::OutOfRange("saved_workspace.workspace_id"));
        }
        if self.resume_detail.len() > MAX_DETAIL_LEN {
            return Err(Reject::OutOfRange("saved_workspace.resume_detail"));
        }
        if self.resume != 0 {
            SavedResumeCode::decode(self.resume, "saved_workspace.resume")?;
        }
        SavedHealthCode::decode(self.health, "saved_workspace.health")?;
        SavedRootCode::decode(self.root_state, "saved_workspace.root_state")?;
        Ok(())
    }
}

/// A node's complete list of saved workspaces; replaces the previous one.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct SavedWorkspaces {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<SavedWorkspace>,
}

/// More saved workspaces than any person keeps; a bound, not a feature.
pub const MAX_SAVED: usize = 1024;

impl Validate for SavedWorkspaces {
    fn validate(&self) -> Result<(), Reject> {
        validate_list(&self.items)
    }
}

pub(crate) fn validate_list(items: &[SavedWorkspace]) -> Result<(), Reject> {
    if items.len() > MAX_SAVED {
        return Err(Reject::OutOfRange("saved_workspaces.items"));
    }
    items.iter().try_for_each(Validate::validate)
}
