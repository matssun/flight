// SPDX-License-Identifier: MIT

use std::fmt;

/// Why a terminal onto a pane on another machine was not opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// The node cannot show terminals at all (it is older, or runs without them).
    Unsupported(String),
    /// Anything else: the pane changed, a limit was reached, the node was not reachable.
    Failed(String),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}
