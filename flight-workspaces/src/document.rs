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
