// SPDX-License-Identifier: MIT

use std::fmt;

/// Id of the rule (or fallback) that produced a classification, e.g. `permit.yn`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleId(String);

/// Id reported when no rule matched but the manifest's prompt marker was on screen.
pub const PROMPT_MARKER_RULE_ID: &str = "idle.prompt";

impl RuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
