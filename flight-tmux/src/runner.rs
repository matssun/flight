// SPDX-License-Identifier: MIT

use crate::{TmuxEndpoint, TmuxError};
use std::process::Command;

/// Captured result of one tmux invocation.
#[derive(Debug, Clone)]
pub struct TmuxOutput {
    pub stdout: String,
}

/// Seam over process spawning so `Tmux` is testable without a tmux server.
pub trait TmuxRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError>;
}

/// Lets a registry hold runners of different kinds behind one type.
impl<R: TmuxRunner + ?Sized> TmuxRunner for Box<R> {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        (**self).run(args)
    }
}

/// Runs the real `tmux` binary against one explicit endpoint.
#[derive(Debug, Clone)]
pub struct SystemRunner {
    endpoint: TmuxEndpoint,
}

impl SystemRunner {
    pub fn new(endpoint: TmuxEndpoint) -> Self {
        Self { endpoint }
    }
}

impl TmuxRunner for SystemRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let out = Command::new("tmux")
            .args(self.endpoint.args())
            .args(args)
            .output()
            .map_err(TmuxError::Spawn)?;
        if out.status.success() {
            Ok(TmuxOutput {
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            })
        } else {
            Err(TmuxError::Failed {
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            })
        }
    }
}
