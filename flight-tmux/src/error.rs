// SPDX-License-Identifier: MIT

use std::fmt;

/// Why a tmux call failed.
#[derive(Debug)]
pub enum TmuxError {
    /// The tmux binary could not be started (not installed, not on PATH).
    Spawn(std::io::Error),
    /// tmux ran and exited non-zero (e.g. no server running).
    Failed { code: Option<i32>, stderr: String },
    /// A socket name that is empty or contains a path separator.
    InvalidEndpoint(String),
}

impl fmt::Display for TmuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "cannot run tmux: {e}"),
            Self::Failed {
                code: Some(c),
                stderr,
            } => write!(f, "tmux exited {c}: {stderr}"),
            Self::InvalidEndpoint(n) => write!(f, "invalid tmux socket name: {n:?}"),
            Self::Failed { code: None, stderr } => write!(f, "tmux killed by signal: {stderr}"),
        }
    }
}

impl std::error::Error for TmuxError {}
