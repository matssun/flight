// SPDX-License-Identifier: MIT

use crate::{ConfigKey, ResumeRef, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

const FILE: &str = "resume.toml";
const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct OnDisk {
    version: u32,
    #[serde(default, rename = "entry")]
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    workspace: ConfigKey,
    surface: ConfigKey,
    #[serde(flatten)]
    reference: ResumeRef,
}

/// Where agents' resume references are kept: next to the saved workspaces, in a file of its own.
///
/// Not in `workspaces.toml` on purpose. That file is a definition: it is exported, shared and
/// imported, and may be read by anyone it is given to. A reference is a key to a conversation
/// history; it is local to the machine and user that made it, readable only by that user
/// (mode 0600), and is not carried by an export. Replaced atomically like the definitions, and
/// a file this build cannot read or that a newer build wrote is left exactly as found.
pub struct ResumeStore {
    path: PathBuf,
    entries: BTreeMap<(ConfigKey, ConfigKey), ResumeRef>,
}

impl ResumeStore {
    /// Load the references in `dir`. A missing file is an empty store; a file that cannot be
    /// read is an error and is left alone.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = dir.into().join(FILE);
        let mut store = Self {
            path,
            entries: BTreeMap::new(),
        };
        let text = match std::fs::read_to_string(&store.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(e.into()),
        };
        let disk: OnDisk = toml::from_str(&text)
            .map_err(|e| StoreError::Io(std::io::Error::other(format!("{FILE}: {e}"))))?;
        if disk.version > SCHEMA {
            return Err(StoreError::NewerOnDisk {
                found: disk.version,
            });
        }
        for e in disk.entries {
            store.entries.insert((e.workspace, e.surface), e.reference);
        }
        Ok(store)
    }

    pub fn get(&self, workspace: &ConfigKey, surface: &ConfigKey) -> Option<&ResumeRef> {
        self.entries.get(&(workspace.clone(), surface.clone()))
    }

    /// Remember the reference for a surface, replacing an earlier one (a replacement agent is a
    /// new session, and the old reference must not outlive the agent it named).
    pub fn put(&mut self, workspace: &ConfigKey, surface: &ConfigKey, reference: ResumeRef) {
        self.entries
            .insert((workspace.clone(), surface.clone()), reference);
    }

    pub fn remove(&mut self, workspace: &ConfigKey, surface: &ConfigKey) -> bool {
        self.entries
            .remove(&(workspace.clone(), surface.clone()))
            .is_some()
    }

    /// Drop every reference of a workspace that is no longer saved.
    pub fn remove_workspace(&mut self, workspace: &ConfigKey) {
        self.entries.retain(|(w, _), _| w != workspace);
    }

    /// Keep only the references of the workspaces in `keep`.
    pub fn retain_workspaces(&mut self, keep: &[ConfigKey]) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(w, _), _| keep.contains(w));
        self.entries.len() != before
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Save, atomically, readable by the owner only.
    pub fn save(&self) -> Result<(), StoreError> {
        let disk = OnDisk {
            version: SCHEMA,
            entries: self
                .entries
                .iter()
                .map(|((w, s), r)| Entry {
                    workspace: w.clone(),
                    surface: s.clone(),
                    reference: r.clone(),
                })
                .collect(),
        };
        let text = toml::to_string_pretty(&disk)
            .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?;
        write_private(&self.path, text.as_bytes())?;
        Ok(())
    }
}

/// [`crate::atomic::write`] for a file only its owner may read.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{FILE}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResumeScope;
    use std::os::unix::fs::PermissionsExt;

    fn key(n: u8) -> ConfigKey {
        ConfigKey::parse(&format!("c-{n}")).unwrap()
    }

    fn reference(token: &str) -> ResumeRef {
        ResumeRef::new(
            "claude",
            token,
            ResumeScope {
                host: "h".into(),
                root: "/r".into(),
                user: "501".into(),
            },
        )
        .unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("flight-resume-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn references_survive_a_save_and_a_load_and_the_file_is_private() {
        let d = dir("roundtrip");
        let mut s = ResumeStore::open(&d).unwrap();
        assert!(s.is_empty());
        s.put(&key(1), &key(2), reference("aaaa-1111"));
        s.put(&key(3), &key(4), reference("bbbb-2222"));
        s.save().unwrap();
        let mode = std::fs::metadata(d.join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let again = ResumeStore::open(&d).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(again.get(&key(1), &key(2)).unwrap().token(), "aaaa-1111");
        assert!(again.get(&key(1), &key(4)).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_newer_or_unreadable_file_is_an_error_and_is_left_as_found() {
        let d = dir("newer");
        std::fs::write(d.join(FILE), "version = 99\n").unwrap();
        assert!(matches!(
            ResumeStore::open(&d),
            Err(StoreError::NewerOnDisk { found: 99 })
        ));
        std::fs::write(d.join(FILE), "not toml {{{").unwrap();
        assert!(ResumeStore::open(&d).is_err());
        assert_eq!(
            std::fs::read_to_string(d.join(FILE)).unwrap(),
            "not toml {{{"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn replacing_and_pruning() {
        let d = dir("prune");
        let mut s = ResumeStore::open(&d).unwrap();
        s.put(&key(1), &key(2), reference("old"));
        s.put(&key(1), &key(2), reference("new"));
        assert_eq!(s.get(&key(1), &key(2)).unwrap().token(), "new");
        s.put(&key(5), &key(6), reference("other"));
        assert!(s.retain_workspaces(&[key(1)]));
        assert_eq!(s.len(), 1);
        s.remove_workspace(&key(1));
        assert!(s.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
