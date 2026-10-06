// SPDX-License-Identifier: MIT

/// Which list the cursor is in. `Tab` switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Panes that need the user, most urgent first.
    Attention,
    /// Every agent pane, grouped by host, in a stable order.
    Tree,
}

impl Section {
    pub fn other(self) -> Self {
        match self {
            Self::Attention => Self::Tree,
            Self::Tree => Self::Attention,
        }
    }
}
