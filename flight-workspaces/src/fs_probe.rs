// SPDX-License-Identifier: MIT

use crate::{GitMarker, RootIdentity, RootProbe, RootState};
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Probes the local filesystem. The host argument is ignored: a node probes its own paths.
pub struct FsProbe {
    home: Option<PathBuf>,
}

impl FsProbe {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }

    fn expand(&self, path: &str) -> Option<PathBuf> {
        match path.strip_prefix('~') {
            Some(rest) => Some(self.home.as_ref()?.join(rest.trim_start_matches('/'))),
            None => Some(PathBuf::from(path)),
        }
    }
}

impl RootProbe for FsProbe {
    fn probe(&self, _host: &str, path: &str) -> RootState {
        let Some(full) = self.expand(path).filter(|p| p.is_absolute()) else {
            return unverified("the path is not absolute and has no home to resolve against");
        };
        match std::fs::metadata(&full) {
            Ok(m) if m.is_dir() => RootState::Present {
                identity: RootIdentity::of(&m),
                git: GitMarker::detect(&full),
            },
            Ok(_) => RootState::NotADirectory,
            Err(e) => match e.kind() {
                ErrorKind::NotFound => absent_or_unverified(&full),
                ErrorKind::PermissionDenied => RootState::PermissionDenied,
                kind => unverified(&format!("cannot look at the path: {kind}")),
            },
        }
    }
}

fn unverified(reason: &str) -> RootState {
    RootState::Unverified {
        reason: reason.to_owned(),
    }
}

/// `NotFound` is only a confirmed absence when the parent is there to say so. A missing parent,
/// or an empty directory sitting on a mount point, is what an unmounted volume looks like.
fn absent_or_unverified(full: &Path) -> RootState {
    let Some(parent) = full.parent() else {
        return unverified("the path has no parent");
    };
    match std::fs::metadata(parent) {
        Ok(m) if m.is_dir() => {
            if looks_unmounted(parent, &m) {
                unverified("the parent is an empty mount point; the volume may be unmounted")
            } else {
                RootState::Missing
            }
        }
        Ok(_) => RootState::NotADirectory,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            unverified("an ancestor is missing; the volume may be unmounted")
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => RootState::PermissionDenied,
        Err(e) => unverified(&format!("cannot look at the parent: {}", e.kind())),
    }
}

fn looks_unmounted(parent: &Path, parent_meta: &std::fs::Metadata) -> bool {
    let on_other_device = parent
        .parent()
        .and_then(|g| std::fs::metadata(g).ok())
        .is_some_and(|g| g.dev() != parent_meta.dev());
    let empty = std::fs::read_dir(parent).is_ok_and(|mut d| d.next().is_none());
    on_other_device && empty
}
