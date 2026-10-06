// SPDX-License-Identifier: MIT

use crate::TmuxError;
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

/// Runs the real `tmux` binary.
#[derive(Debug, Default, Clone)]
pub struct SystemRunner;

impl TmuxRunner for SystemRunner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let out = Command::new("tmux")
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
