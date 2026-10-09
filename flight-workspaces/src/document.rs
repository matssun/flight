// SPDX-License-Identifier: MIT

use crate::{migrate, LoadError, Profile};
use serde::{Deserialize, Serialize};

/// The schema this build writes. Raised only with a migration step from the previous one.
pub const CURRENT_SCHEMA: u32 = 1;

pub const DEFAULT_PROFILE: &str = "default";

/// Everything saved for one host: versioned, structured, human-readable TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub active_profile: String,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<Profile>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA,
            active_profile: DEFAULT_PROFILE.to_owned(),
            profiles: vec![Profile::new(DEFAULT_PROFILE)],
        }
    }
}

impl Document {
    pub fn active(&self) -> Option<&Profile> {
        self.profile(&self.active_profile)
    }

    pub fn active_mut(&mut self) -> Option<&mut Profile> {
        let name = self.active_profile.clone();
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Make `name` the active profile. False if there is no such profile.
    pub fn use_profile(&mut self, name: &str) -> bool {
        let known = self.profile(name).is_some();
        if known {
            self.active_profile = name.to_owned();
        }
        known
    }

    /// Add a profile under its own name. Names are plain text and unique; an existing profile is
    /// never replaced.
    pub fn add_profile(&mut self, profile: Profile) -> Result<(), ProfileError> {
        if !valid_profile_name(&profile.name) {
            return Err(ProfileError::BadName);
        }
        if self.profile(&profile.name).is_some() {
            return Err(ProfileError::Exists);
        }
        self.profiles.push(profile);
        Ok(())
    }

    /// Copy `from` as `to`, with new identities (see [`Profile::duplicate`]).
    pub fn duplicate_profile(&mut self, from: &str, to: &str) -> Result<(), ProfileError> {
        let copy = self
            .profile(from)
            .ok_or(ProfileError::Unknown)?
            .duplicate(to)
            .map_err(|_| ProfileError::NoRandomness)?;
        self.add_profile(copy)
    }

    pub fn to_toml(&self) -> Result<String, std::io::Error> {
        toml::to_string_pretty(self).map_err(std::io::Error::other)
    }

    /// Parse and, if older, migrate. A document from a newer Flight is refused rather than
    /// read: reading it and writing it back would silently drop what this build cannot see.
    pub fn from_toml(text: &str) -> Result<Self, LoadError> {
        Self::from_toml_with(text, migrate::STEPS)
    }

    pub(crate) fn from_toml_with(text: &str, steps: &[migrate::Step]) -> Result<Self, LoadError> {
        let mut table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| LoadError::Corrupt(e.to_string()))?;
        let found = table
            .get("schema_version")
            .and_then(toml::Value::as_integer)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v >= 1)
            .ok_or_else(|| LoadError::Corrupt("missing or invalid schema_version".to_owned()))?;
        if usize::try_from(found).map_or(true, |f| f > steps.len().saturating_add(1)) {
            return Err(LoadError::TooNew { found });
        }
        migrate::run(&mut table, found, steps)?;
        table
            .try_into()
            .map_err(|e: toml::de::Error| LoadError::Corrupt(e.to_string()))
    }
}

/// Why a profile change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileError {
    BadName,
    Exists,
    Unknown,
    NoRandomness,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadName => "a profile name is letters, digits, '_', '-' and '.'",
            Self::Exists => "a profile with that name exists",
            Self::Unknown => "no such profile",
            Self::NoRandomness => "cannot make new identities here",
        })
    }
}

impl std::error::Error for ProfileError {}

fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}
