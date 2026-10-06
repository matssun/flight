// SPDX-License-Identifier: MIT

use crate::TmuxError;
use std::path::PathBuf;

/// Which tmux server to talk to. There is deliberately no default: Flight never
/// assumes the user's own tmux server (ADR-001, "Explicit endpoints").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEndpoint {
    /// `tmux -L name`: a named socket in tmux's per-user socket directory.
    Named(String),
    /// `tmux -S path`: an explicit socket path.
    Path(PathBuf),
}

impl TmuxEndpoint {
    /// A named socket. The name must be non-empty and contain no path separator.
    pub fn named(name: &str) -> Result<Self, TmuxError> {
        if name.is_empty() || name.contains('/') {
            return Err(TmuxError::InvalidEndpoint(name.to_owned()));
        }
        Ok(Self::Named(name.to_owned()))
    }

    /// Leading tmux arguments selecting this server.
    pub fn args(&self) -> [String; 2] {
        match self {
            Self::Named(n) => ["-L".to_owned(), n.clone()],
            Self::Path(p) => ["-S".to_owned(), p.to_string_lossy().into_owned()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_selects_with_dash_l() {
        assert_eq!(
            TmuxEndpoint::named("flight").unwrap().args(),
            ["-L", "flight"]
        );
    }

    #[test]
    fn path_selects_with_dash_s() {
        assert_eq!(TmuxEndpoint::Path("/tmp/s".into()).args(), ["-S", "/tmp/s"]);
    }

    #[test]
    fn rejects_empty_and_slashed_names() {
        assert!(TmuxEndpoint::named("").is_err());
        assert!(TmuxEndpoint::named("a/b").is_err());
    }
}
