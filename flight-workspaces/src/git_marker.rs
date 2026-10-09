// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

/// What a root's `.git` entry looks like. Observed from the filesystem only: Flight never runs
/// `git`, never requires a repository, and never assumes `.git` is a directory. A linked
/// worktree or a submodule has a `.git` *file* pointing elsewhere; that is recorded as such and
/// its target is kept as text, not followed, so a later worktree feature can build on it
/// without changing what a workspace is (ADR-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GitMarker {
    /// No `.git` entry: an ordinary directory.
    Absent,
    /// A `.git` directory: an independent clone (or a bare-style layout inside the root).
    Directory,
    /// A `.git` file. `gitdir` is the `gitdir:` line when there is a valid one; `None` means
    /// the file is not a pointer Flight understands (invalid metadata, reported, never repaired).
    File { gitdir: Option<String> },
    /// The entry exists but could not be read. Observed only; never stored as a record.
    Unreadable,
}

const POINTER_LIMIT: u64 = 4096;

impl GitMarker {
    /// Look at `root/.git` without following it.
    pub fn detect(root: &Path) -> Self {
        let entry = root.join(".git");
        let meta = match std::fs::symlink_metadata(&entry) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::Absent,
            Err(_) => return Self::Unreadable,
        };
        if meta.is_dir() {
            return Self::Directory;
        }
        let mut text = String::new();
        let read = std::fs::File::open(&entry)
            .and_then(|f| f.take(POINTER_LIMIT).read_to_string(&mut text));
        if read.is_err() {
            return Self::File { gitdir: None };
        }
        let gitdir = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("gitdir:"))
            .map(|g| g.trim().to_owned())
            .filter(|g| !g.is_empty());
        Self::File { gitdir }
    }

    /// Whether going from `recorded` to `self` is a change of layout worth the user's notice
    /// (a clone that became a pointer file, or the reverse).
    pub fn layout_differs(&self, recorded: &Self) -> bool {
        std::mem::discriminant(self) != std::mem::discriminant(recorded)
    }
}
