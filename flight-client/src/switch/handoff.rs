// SPDX-License-Identifier: MIT

use std::os::unix::process::CommandExt;
use std::process::Command;

/// A program that takes over the terminal once the dashboard has exited: an attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    pub program: String,
    pub args: Vec<String>,
}

impl Handoff {
    /// Replace this process. Returns only if that failed.
    pub fn exec(self) -> std::io::Error {
        Command::new(&self.program).args(&self.args).exec()
    }
}
