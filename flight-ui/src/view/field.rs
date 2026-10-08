// SPDX-License-Identifier: MIT

/// The things in the new-session form that can have focus, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Host,
    Name,
    Directory,
    Start,
    Create,
    Cancel,
}

impl Field {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Host => Self::Name,
            Self::Name => Self::Directory,
            Self::Directory => Self::Start,
            Self::Start => Self::Create,
            Self::Create => Self::Cancel,
            Self::Cancel => Self::Host,
        }
    }

    pub(super) fn prev(self) -> Self {
        match self {
            Self::Host => Self::Cancel,
            Self::Name => Self::Host,
            Self::Directory => Self::Name,
            Self::Start => Self::Directory,
            Self::Create => Self::Start,
            Self::Cancel => Self::Create,
        }
    }
}
