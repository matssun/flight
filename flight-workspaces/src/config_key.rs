// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

/// The stable identity of a saved workspace or surface. It is minted once, written to the saved
/// definition, and never reused. It is not a `WorkspaceId`, `SurfaceId`, tmux id, pid or
/// connection: those describe a running thing and change when it restarts; this names the
/// user's intent and does not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ConfigKey(String);

impl ConfigKey {
    /// A fresh key such as `c-3f9c1a2b4d5e6f70`: 64 random bits from the operating system.
    pub fn mint() -> Result<Self, std::io::Error> {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes)
            .map_err(|_| std::io::Error::other("no randomness available"))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self(format!("c-{hex}")))
    }

    /// A key read from outside, accepted only if it is plain text of a sane length (it is
    /// placed in backend metadata and in logs).
    pub fn parse(text: &str) -> Option<Self> {
        let ok = !text.is_empty()
            && text.len() <= 64
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        ok.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ConfigKey {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("invalid key {value:?}"))
    }
}

impl From<ConfigKey> for String {
    fn from(key: ConfigKey) -> Self {
        key.0
    }
}

impl std::fmt::Display for ConfigKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
