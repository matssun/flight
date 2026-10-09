// SPDX-License-Identifier: MIT

/// How small a tile may get before the layout gives up showing everything at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub min_cols: u16,
    pub min_rows: u16,
}

impl Default for Style {
    /// Enough for a prompt and a few words; below this a terminal program is not usable.
    fn default() -> Self {
        Self {
            min_cols: 20,
            min_rows: 5,
        }
    }
}
