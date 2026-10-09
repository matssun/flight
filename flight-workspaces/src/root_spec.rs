// SPDX-License-Identifier: MIT

use crate::{GitMarker, RootIdentity, RootState};
use serde::{Deserialize, Serialize};

/// The saved root of a workspace: where it is, and what it was when last seen. Plain
/// directories and independent clones are both just roots; `git` is recorded metadata, not a
/// requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSpec {
    /// Absolute, or `~`-relative to the owning host's home.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RootIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitMarker>,
}

/// How an observation compares with what was saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootCheck {
    /// Present, and the same directory that was recorded.
    Verified,
    /// Present, nothing was recorded yet: the first sight, which `record` can remember.
    FirstSighting,
    /// Present, but not demonstrably the saved directory. Needs the user's decision.
    Changed(String),
    /// Not usable now; the saved definition is untouched.
    Unavailable(RootState),
}

impl RootCheck {
    /// Whether a process may be started in the root.
    pub fn usable(&self) -> bool {
        matches!(self, Self::Verified | Self::FirstSighting)
    }
}

impl RootSpec {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            identity: None,
            git: None,
        }
    }

    pub fn check(&self, observed: &RootState) -> RootCheck {
        let RootState::Present { identity, git } = observed else {
            return RootCheck::Unavailable(observed.clone());
        };
        let Some(recorded) = self.identity else {
            return RootCheck::FirstSighting;
        };
        if recorded != *identity {
            return RootCheck::Changed(format!(
                "a different directory is at this path now (was device {} inode {}, is device {} \
                 inode {})",
                recorded.dev, recorded.ino, identity.dev, identity.ino
            ));
        }
        match &self.git {
            Some(saved) if git.layout_differs(saved) => RootCheck::Changed(
                "the repository layout of the directory changed since it was saved".to_owned(),
            ),
            _ => RootCheck::Verified,
        }
    }

    /// Remember what a present root looks like. Only a present root can be remembered.
    pub fn record(&mut self, observed: &RootState) {
        if let RootState::Present { identity, git } = observed {
            self.identity = Some(*identity);
            self.git = (*git != GitMarker::Unreadable).then(|| git.clone());
        }
    }
}
