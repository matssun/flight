// SPDX-License-Identifier: MIT

use std::fmt;

/// Why a message or connection was refused. Always about the peer's bytes, never a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// Peers speak different protocol majors.
    MajorMismatch { ours: u32, theirs: u32 },
    /// An enum field held `Unspecified` or a value this build does not know.
    UnknownEnum { field: &'static str, value: i32 },
    /// A required field or oneof body was absent.
    Missing(&'static str),
    /// A required string was empty.
    Empty(&'static str),
    /// A numeric field was outside its allowed range.
    OutOfRange(&'static str),
    /// The encoded message exceeds the frame size limit.
    TooLarge { len: usize, max: usize },
    /// The bytes are not a valid protobuf message.
    Malformed(String),
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MajorMismatch { ours, theirs } => {
                write!(f, "protocol major mismatch (ours {ours}, theirs {theirs})")
            }
            Self::UnknownEnum { field, value } => {
                write!(f, "unknown or unspecified value {value} for {field}")
            }
            Self::Missing(what) => write!(f, "missing {what}"),
            Self::Empty(what) => write!(f, "empty {what}"),
            Self::OutOfRange(what) => write!(f, "{what} out of range"),
            Self::TooLarge { len, max } => write!(f, "frame of {len} bytes exceeds {max}"),
            Self::Malformed(m) => write!(f, "malformed message: {m}"),
        }
    }
}

impl std::error::Error for Reject {}
