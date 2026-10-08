// SPDX-License-Identifier: MIT

use crate::TmuxError;
use std::fmt;

/// Why a session was not created. In every case no session of the requested name was left
/// behind by this call.
#[derive(Debug)]
pub enum CreateError {
    /// A session of that name existed already; it was not touched.
    AlreadyExists,
    /// The session came up and its program ended at once (not found, not executable, crashed
    /// on start). The session is gone.
    Exited,
    Tmux(TmuxError),
}

impl fmt::Display for CreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists => write!(f, "a session with that name already exists"),
            Self::Exited => write!(f, "the program exited as soon as it started"),
            Self::Tmux(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CreateError {}

impl From<TmuxError> for CreateError {
    fn from(e: TmuxError) -> Self {
        Self::Tmux(e)
    }
}
