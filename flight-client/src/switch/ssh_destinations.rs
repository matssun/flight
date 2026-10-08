// SPDX-License-Identifier: MIT

use flight_control::SshRunner;
use flight_state::HostId;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct SshDestinationsError(String);

impl fmt::Display for SshDestinationsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    ssh: String,
    /// For the operator's own reference. Never consulted.
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    nodes: HashMap<String, Entry>,
}

/// Operator-owned map from a node's stable id to the ssh destination used to attach to it
/// (`ui/ssh.toml`). Keyed by [`HostId`] only: a name a node advertises is data, never a
/// route. Destinations are used exactly as written, after the same check every ssh
/// destination gets.
///
/// ```toml
/// [nodes."<HostId>"]
/// ssh = "mini-2"
/// name = "the mac mini"   # optional, display only
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshDestinations {
    by_host: HashMap<HostId, String>,
}

impl SshDestinations {
    pub fn parse(text: &str) -> Result<Self, SshDestinationsError> {
        let file: File =
            toml::from_str(text).map_err(|e| SshDestinationsError(format!("ssh.toml: {e}")))?;
        let mut by_host = HashMap::new();
        for (id, entry) in file.nodes {
            SshRunner::check_alias(&entry.ssh).map_err(|_| {
                SshDestinationsError(format!("ssh.toml: unusable ssh destination for node {id}"))
            })?;
            by_host.insert(HostId::new(id), entry.ssh);
        }
        Ok(Self { by_host })
    }

    /// A missing file is an empty map; an unreadable or invalid one is an error.
    pub fn load(path: &Path) -> Result<Self, SshDestinationsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(SshDestinationsError(format!("{}: {e}", path.display()))),
        }
    }

    /// Where the file lives under a UI role directory.
    pub fn path_in(ui_dir: &Path) -> std::path::PathBuf {
        ui_dir.join("ssh.toml")
    }

    pub fn get(&self, host: &HostId) -> Option<&str> {
        self.by_host.get(host).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_node_id_to_its_destination() {
        let d =
            SshDestinations::parse("[nodes.\"abc\"]\nssh = \"mini-2\"\nname = \"mini\"\n").unwrap();
        assert_eq!(d.get(&HostId::new("abc")), Some("mini-2"));
        assert_eq!(d.get(&HostId::new("mini")), None, "names are not keys");
    }

    #[test]
    fn an_empty_file_and_a_missing_file_map_nothing() {
        assert_eq!(
            SshDestinations::parse("").unwrap(),
            SshDestinations::default()
        );
        let missing = Path::new("/nonexistent-flight-test/ssh.toml");
        assert_eq!(
            SshDestinations::load(missing).unwrap(),
            SshDestinations::default()
        );
    }

    #[test]
    fn a_destination_that_could_be_an_option_is_rejected() {
        for bad in ["-oProxyCommand=x", "a b", "", "a\\nb"] {
            let text = format!("[nodes.\"abc\"]\nssh = \"{bad}\"\n");
            assert!(SshDestinations::parse(&text).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unknown_fields_and_missing_destinations_are_errors() {
        assert!(SshDestinations::parse("[nodes.\"a\"]\nhost = \"x\"\nssh = \"y\"\n").is_err());
        assert!(SshDestinations::parse("[nodes.\"a\"]\nname = \"x\"\n").is_err());
        assert!(SshDestinations::parse("[nods.\"a\"]\nssh = \"y\"\n").is_err());
    }
}
