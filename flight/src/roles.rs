// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

/// Each role keeps its own identity and settings: an orchestrator, a node and a UI on one
/// machine are three separate identities, as they would be on three machines.
pub fn orchestrator_dir(base: &Path) -> PathBuf {
    base.join("orchestrator")
}

pub fn node_dir(base: &Path) -> PathBuf {
    base.join("node")
}

pub fn ui_dir(base: &Path) -> PathBuf {
    base.join("ui")
}
