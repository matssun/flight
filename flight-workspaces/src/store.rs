// SPDX-License-Identifier: MIT

use crate::atomic::{self, TMP_MARK};
use crate::migrate::{self, Step};
use crate::{Document, LoadError, StoreError};
use std::path::PathBuf;

const FILE: &str = "workspaces.toml";
const SNAPSHOTS: &str = "snapshots";

/// How [`Store::load`] arrived at its document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// No file yet: an empty document, nothing written.
    Fresh,
    Existing,
    /// Upgraded from an older schema. The original is kept as `workspaces.v<from>.bak`.
    Migrated {
        from: u32,
    },
}

/// The saved workspaces of one host: one file, owned by the node that owns the roots, written
/// only through atomic replacement. A file that cannot be read is never replaced or moved by
/// this type; the caller reports it and the user decides.
pub struct Store {
    dir: PathBuf,
    steps: &'static [Step],
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_steps(dir, migrate::STEPS)
    }

    pub(crate) fn with_steps(dir: impl Into<PathBuf>, steps: &'static [Step]) -> Self {
        Self {
            dir: dir.into(),
            steps,
        }
    }

    fn target(&self) -> u32 {
        u32::try_from(self.steps.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1)
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    pub fn load(&self) -> Result<(Document, Loaded), StoreError> {
        let text = match std::fs::read_to_string(self.path()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Document::default(), Loaded::Fresh))
            }
            Err(e) => return Err(e.into()),
        };
        let doc = Document::from_toml_with(&text, self.steps)?;
        let from = stored_version(&text);
        match from {
            Some(v) if v < self.target() => {
                atomic::write(
                    &self.dir.join(format!("workspaces.v{v}.bak")),
                    text.as_bytes(),
                )?;
                self.save(&doc)?;
                Ok((doc, Loaded::Migrated { from: v }))
            }
            _ => Ok((doc, Loaded::Existing)),
        }
    }

    /// Persist `doc`. Refuses to overwrite a file written by a newer Flight.
    pub fn save(&self, doc: &Document) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.dir)?;
        if let Ok(existing) = std::fs::read_to_string(self.path()) {
            if let Some(found) = stored_version(&existing).filter(|v| *v > self.target()) {
                return Err(StoreError::NewerOnDisk { found });
            }
            // A file that cannot be read is the user's to look at, not ours to overwrite.
            if stored_version(&existing).is_none() {
                return Err(LoadError::Corrupt("the saved file is unreadable".to_owned()).into());
            }
        }
        Ok(atomic::write(&self.path(), doc.to_toml()?.as_bytes())?)
    }

    /// Move an unreadable saved file aside, intact, as `workspaces.corrupt-<n>.toml`, so a fresh
    /// one can be started. Only ever called on the user's say-so.
    pub fn quarantine(&self) -> Result<PathBuf, StoreError> {
        for n in 0u32..1000 {
            let to = self.dir.join(format!("workspaces.corrupt-{n}.toml"));
            if !to.exists() {
                std::fs::rename(self.path(), &to)?;
                return Ok(to);
            }
        }
        Err(StoreError::Exists)
    }

    /// Remove temporary files a crash left behind. They are never read, so this is housekeeping.
    pub fn sweep_temporaries(&self) -> std::io::Result<usize> {
        let mut removed = 0usize;
        for dir in [self.dir.clone(), self.dir.join(SNAPSHOTS)] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().contains(TMP_MARK)
                    && std::fs::remove_file(entry.path()).is_ok()
                {
                    removed = removed.saturating_add(1);
                }
            }
        }
        Ok(removed)
    }

    /// Keep a named copy of `doc`. A snapshot is never overwritten.
    pub fn snapshot(&self, label: &str, doc: &Document) -> Result<PathBuf, StoreError> {
        let path = self.snapshot_path(label)?;
        if path.exists() {
            return Err(StoreError::Exists);
        }
        std::fs::create_dir_all(self.dir.join(SNAPSHOTS))?;
        atomic::write(&path, doc.to_toml()?.as_bytes())?;
        Ok(path)
    }

    pub fn load_snapshot(&self, label: &str) -> Result<Document, StoreError> {
        match std::fs::read_to_string(self.snapshot_path(label)?) {
            Ok(t) => Ok(Document::from_toml(&t)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    /// Make a snapshot the saved state, after keeping the current state under `backup_label`.
    pub fn restore_snapshot(
        &self,
        label: &str,
        backup_label: &str,
    ) -> Result<Document, StoreError> {
        let restored = self.load_snapshot(label)?;
        let (current, _) = self.load()?;
        self.snapshot(backup_label, &current)?;
        self.save(&restored)?;
        Ok(restored)
    }

    pub fn list_snapshots(&self) -> Vec<String> {
        let mut labels: Vec<String> = std::fs::read_dir(self.dir.join(SNAPSHOTS))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".toml").map(str::to_owned)
            })
            .collect();
        labels.sort();
        labels
    }

    fn snapshot_path(&self, label: &str) -> Result<PathBuf, StoreError> {
        let plain = !label.is_empty()
            && label.len() <= 64
            && !label.starts_with('.')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        if !plain {
            return Err(StoreError::BadLabel);
        }
        Ok(self.dir.join(SNAPSHOTS).join(format!("{label}.toml")))
    }
}

fn stored_version(text: &str) -> Option<u32> {
    let table: toml::Table = text.parse().ok()?;
    let v = table.get("schema_version")?.as_integer()?;
    u32::try_from(v).ok()
}
