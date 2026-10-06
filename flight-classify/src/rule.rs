// SPDX-License-Identifier: MIT

use crate::{ClassifyError, RuleId};
use flight_state::AgentState;
use regex::Regex;

/// One ordered detection rule: if `pattern` matches the text, the state is `state`.
/// Lower `priority` is evaluated first; the first match wins.
#[derive(Debug, Clone)]
pub struct Rule {
    id: RuleId,
    priority: u16,
    pattern: Regex,
    state: AgentState,
}

impl Rule {
    /// `pattern` is a Rust regex; use inline `(?i)` / `(?m)` for case-insensitive /
    /// multi-line matching.
    pub fn new(
        id: &str,
        priority: u16,
        pattern: &str,
        state: AgentState,
    ) -> Result<Self, ClassifyError> {
        let pattern = Regex::new(pattern).map_err(|e| ClassifyError::BadPattern {
            rule: id.to_owned(),
            message: e.to_string(),
        })?;
        Ok(Self {
            id: RuleId::new(id),
            priority,
            pattern,
            state,
        })
    }

    pub fn id(&self) -> &RuleId {
        &self.id
    }

    pub fn priority(&self) -> u16 {
        self.priority
    }

    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.pattern.is_match(text)
    }
}
