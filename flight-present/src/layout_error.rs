// SPDX-License-Identifier: MIT

/// Why a layout was refused. A layout read from outside is checked, not trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// No surface at all.
    Empty,
    /// A split or tab set with nothing in it.
    EmptyRegion,
    /// A weight of zero or beyond the limit.
    BadWeight,
    /// A tab index that is not a tab.
    BadTab,
    /// The same surface in two places: it would need two attachments.
    Duplicate(String),
    TooDeep,
    TooMany,
    /// The focus names a surface that is not showing.
    FocusNotShowing,
    /// A surface that is not in the layout.
    Unknown(String),
    /// A layout that cannot be read (saved form).
    Unreadable(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("the layout shows nothing"),
            Self::EmptyRegion => f.write_str("a split or tab set has nothing in it"),
            Self::BadWeight => f.write_str("a share must be between 1 and the limit"),
            Self::BadTab => f.write_str("the active tab is not a tab"),
            Self::Duplicate(s) => write!(f, "surface {s} is in the layout twice"),
            Self::TooDeep => f.write_str("regions are nested too deeply"),
            Self::TooMany => f.write_str("too many surfaces in one layout"),
            Self::FocusNotShowing => f.write_str("the focused surface is not showing"),
            Self::Unknown(s) => write!(f, "surface {s} is not in the layout"),
            Self::Unreadable(why) => write!(f, "the saved layout cannot be read: {why}"),
        }
    }
}

impl std::error::Error for LayoutError {}
