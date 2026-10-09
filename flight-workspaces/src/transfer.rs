// SPDX-License-Identifier: MIT

use crate::{ConfigKey, Document, LoadError, Origin, Profile, RootSpec, CURRENT_SCHEMA};
use serde::{Deserialize, Serialize};

/// The host label in an exported file: a definition belongs to no machine until it is imported
/// onto one.
pub const PORTABLE_HOST: &str = "-";

/// The largest file an import reads. Definitions are small; this is a bound, not a feature.
pub const MAX_IMPORT_BYTES: usize = 1024 * 1024;

/// What an import did and did not take over, for the user to read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub workspaces: usize,
    /// Agents saved as running without permission prompts. The flag is dropped: a file cannot
    /// grant that.
    pub prompts_restored: usize,
    /// Definitions skipped because they could not be understood, with why.
    pub skipped: Vec<String>,
}

/// A profile as a shareable file. Only the declaration travels: names, roots, surfaces. No
/// runtime id, recorded directory identity, dismissal list, host, or anything else that is
/// about one machine or one moment; and nothing that could carry a secret, since a definition
/// has no field for one.
pub fn export(profile: &Profile) -> Result<String, std::io::Error> {
    let mut shared = Profile::new(profile.name.clone());
    for w in &profile.workspaces {
        let mut w = w.clone();
        w.host = PORTABLE_HOST.to_owned();
        w.last_workspace_id = None;
        w.root = RootSpec::new(w.root.path);
        for s in &mut w.surfaces {
            s.last_surface_id = None;
        }
        shared.workspaces.push(w);
    }
    Document {
        active_profile: shared.name.clone(),
        profiles: vec![shared],
        ..Document::default()
    }
    .to_toml()
}

/// Read a shared file as a profile for `host`, treating it as untrusted. Whatever the file
/// says, the result is a set of *imported* definitions:
///
/// - every saved identity is minted afresh, so a file can neither collide with nor impersonate
///   anything already saved;
/// - they belong to `host`, whatever host the file named;
/// - runtime ids, recorded directory identities and permission-free agents are not taken over;
/// - they start nothing until the user trusts them, and then only on an explicit restore.
pub fn import(text: &str, host: &str, name: &str) -> Result<(Profile, ImportReport), LoadError> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err(LoadError::Corrupt("the file is too large".to_owned()));
    }
    let doc = Document::from_toml(text)?;
    if doc.schema_version > CURRENT_SCHEMA {
        return Err(LoadError::TooNew {
            found: doc.schema_version,
        });
    }
    let source = doc
        .active()
        .or_else(|| doc.profiles.first())
        .ok_or_else(|| LoadError::Corrupt("the file holds no profile".to_owned()))?;
    let mut profile = Profile::new(name);
    let mut report = ImportReport::default();
    for w in &source.workspaces {
        if !flight_state::valid_session_name(&w.name) || !flight_state::valid_dir(&w.root.path) {
            report
                .skipped
                .push(format!("{:?}: not a usable name or directory", w.name));
            continue;
        }
        let mut w = w.clone();
        let fresh = |e: std::io::Error| LoadError::Corrupt(e.to_string());
        w.key = ConfigKey::mint().map_err(fresh)?;
        w.host = host.to_owned();
        w.origin = Origin::Imported;
        w.last_workspace_id = None;
        w.root = RootSpec::new(w.root.path.clone());
        for s in &mut w.surfaces {
            s.key = ConfigKey::mint().map_err(fresh)?;
            s.last_surface_id = None;
            if std::mem::take(&mut s.skip_permissions) {
                report.prompts_restored = report.prompts_restored.saturating_add(1);
            }
        }
        profile.workspaces.push(w);
        report.workspaces = report.workspaces.saturating_add(1);
    }
    Ok((profile, report))
}
