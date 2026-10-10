// SPDX-License-Identifier: MIT

use crate::switch::Refusal;
use std::fmt;

/// A switch that did not complete, by stage. `Refused`, `Reveal` and `Open` changed nothing the
/// user can see; `Present` means the pane *was* selected and only showing it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchError {
    Refused(Refusal),
    Reveal(String),
    /// A terminal onto a pane on another machine was not opened; nothing was selected.
    Open(String),
    Present(String),
}

impl fmt::Display for SwitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => r.fmt(f),
            Self::Reveal(why) => write!(f, "could not select the pane: {why}"),
            Self::Open(why) => write!(f, "cannot open a terminal: {why}"),
            Self::Present(why) => write!(f, "pane selected, but cannot attach: {why}"),
        }
    }
}

impl From<Refusal> for SwitchError {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}
