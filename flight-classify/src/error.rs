// SPDX-License-Identifier: MIT

use std::fmt;

#[derive(Debug)]
pub enum ClassifyError {
    /// A rule's pattern did not compile.
    BadPattern { rule: String, message: String },
    /// Two rules in one list share an id.
    DuplicateId(String),
    /// Two rules in one list share a priority, so their order would be ambiguous.
    DuplicatePriority(u16),
}

impl fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPattern { rule, message } => {
                write!(f, "rule {rule:?}: bad pattern: {message}")
            }
            Self::DuplicateId(id) => write!(f, "duplicate rule id {id:?}"),
            Self::DuplicatePriority(p) => write!(f, "duplicate rule priority {p}"),
        }
    }
}

impl std::error::Error for ClassifyError {}
