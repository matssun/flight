// SPDX-License-Identifier: MIT

use std::ffi::OsString;
use std::path::PathBuf;

/// The node's own environment, which is authoritative for what a new session may start:
/// where `claude` is found and what `~` means. Held as a value so tests can give a node a
/// fake `PATH` without touching the process environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEnv {
    pub home: Option<PathBuf>,
    pub path: OsString,
}

impl SessionEnv {
    /// The environment of this process, which for `flight node run` is the service's.
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from),
            path: std::env::var_os("PATH").unwrap_or_default(),
        }
    }
}

impl Default for SessionEnv {
    fn default() -> Self {
        Self::from_process()
    }
}
