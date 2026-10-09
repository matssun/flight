// SPDX-License-Identifier: MIT

use flight_ui::WorkspaceKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

const FILE: &str = "layouts.toml";
const SCHEMA: u32 = 1;
/// Layouts kept; saving one more forgets the one that sorts first.
const MAX_LAYOUTS: usize = 64;

#[derive(Serialize, Deserialize)]
struct OnDisk {
    version: u32,
    #[serde(default, rename = "layout")]
    layouts: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    host: String,
    workspace: String,
    /// A layout in `flight_present::saved` form.
    text: String,
}

/// How the user last arranged each workspace's surfaces, kept by the UI on this machine.
///
/// A layout says how things are shown, not what they are: it is not part of a saved workspace
/// definition, is not exported with one, and naming a surface in it creates nothing. A file
/// this build cannot read, or that a newer build wrote, is an error and is left exactly as
/// found; one that cannot be saved is reported and the arrangement is simply not remembered.
pub struct LayoutStore {
    path: PathBuf,
    entries: BTreeMap<(String, String), String>,
}

impl LayoutStore {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, String> {
        let path = dir.into().join(FILE);
        let mut store = Self {
            path,
            entries: BTreeMap::new(),
        };
        let text = match std::fs::read_to_string(&store.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(format!("{FILE}: {e}")),
        };
        let disk: OnDisk = toml::from_str(&text).map_err(|e| format!("{FILE}: {e}"))?;
        if disk.version > SCHEMA {
            return Err(format!("{FILE} was written by a newer Flight"));
        }
        for e in disk.layouts.into_iter().take(MAX_LAYOUTS) {
            store.entries.insert((e.host, e.workspace), e.text);
        }
        Ok(store)
    }

    pub fn get(&self, key: &WorkspaceKey) -> Option<&str> {
        self.entries.get(&names(key)).map(String::as_str)
    }

    pub fn put(&mut self, key: &WorkspaceKey, text: String) {
        let name = names(key);
        if !self.entries.contains_key(&name) && self.entries.len() >= MAX_LAYOUTS {
            if let Some(first) = self.entries.keys().next().cloned() {
                self.entries.remove(&first);
            }
        }
        self.entries.insert(name, text);
    }

    /// Save, atomically, readable by the owner only.
    pub fn save(&self) -> Result<(), String> {
        let disk = OnDisk {
            version: SCHEMA,
            layouts: self
                .entries
                .iter()
                .map(|((host, workspace), text)| Entry {
                    host: host.clone(),
                    workspace: workspace.clone(),
                    text: text.clone(),
                })
                .collect(),
        };
        let text = toml::to_string_pretty(&disk).map_err(|e| e.to_string())?;
        write_private(&self.path, text.as_bytes()).map_err(|e| format!("{FILE}: {e}"))
    }
}

fn names(key: &WorkspaceKey) -> (String, String) {
    (key.host.to_string(), key.workspace.to_string())
}

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
    use flight_state::{HostId, WorkspaceId};

    fn key(w: &str) -> WorkspaceKey {
        WorkspaceKey {
            host: HostId::new("h"),
            workspace: WorkspaceId::new(w),
        }
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("flight-layouts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn what_is_saved_is_read_back_privately_and_keyed_by_host_and_workspace() {
        let d = dir("roundtrip");
        let mut store = LayoutStore::open(&d).unwrap();
        assert!(store.get(&key("a")).is_none());
        store.put(&key("a"), "one".to_owned());
        store.put(&key("b"), "two".to_owned());
        store.save().unwrap();
        let back = LayoutStore::open(&d).unwrap();
        assert_eq!(back.get(&key("a")), Some("one"));
        assert_eq!(back.get(&key("b")), Some("two"));
        let mode = std::fs::metadata(d.join(FILE)).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreadable_or_newer_file_is_an_error_and_is_left_alone() {
        let d = dir("bad");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(FILE), "version = 99\n").unwrap();
        assert!(LayoutStore::open(&d).is_err());
        std::fs::write(d.join(FILE), "not toml [").unwrap();
        assert!(LayoutStore::open(&d).is_err());
        assert_eq!(std::fs::read_to_string(d.join(FILE)).unwrap(), "not toml [");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_store_is_bounded() {
        let d = dir("bound");
        let mut store = LayoutStore::open(&d).unwrap();
        for i in 0..(MAX_LAYOUTS + 10) {
            store.put(&key(&format!("w{i:03}")), "x".to_owned());
        }
        assert_eq!(store.entries.len(), MAX_LAYOUTS);
    }
}
