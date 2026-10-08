// SPDX-License-Identifier: MIT

use crate::switch::Refusal;
use std::fmt;

/// A switch that did not complete, by stage. `Reveal` and `Refused` changed nothing the user
/// can see; `Present` means the pane *was* revealed and only showing it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchError {
    Refused(Refusal),
    Reveal(String),
    Present(String),
}

impl fmt::Display for SwitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => r.fmt(f),
            Self::Reveal(why) => write!(f, "could not select the pane: {why}"),
            Self::Present(why) => write!(f, "pane selected, but cannot attach: {why}"),
        }
    }
}

impl From<Refusal> for SwitchError {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}
