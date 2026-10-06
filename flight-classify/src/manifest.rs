// SPDX-License-Identifier: MIT

use crate::{AgentKind, ClassifyError, Rule};
use std::collections::HashSet;

/// Detection rules for one agent. Rule order is by `priority`, not by construction order.
#[derive(Debug, Clone)]
pub struct Manifest {
    agent: AgentKind,
    lines_from_bottom: usize,
    prompt_marker: Option<String>,
    screen_rules: Vec<Rule>,
    title_rules: Vec<Rule>,
}

impl Manifest {
    /// Builds a manifest, ordering each rule list by priority. Duplicate ids or priorities
    /// within a list are rejected so ordering is never ambiguous.
    pub fn new(
        agent: AgentKind,
        lines_from_bottom: usize,
        prompt_marker: Option<&str>,
        screen_rules: Vec<Rule>,
        title_rules: Vec<Rule>,
    ) -> Result<Self, ClassifyError> {
        Ok(Self {
            agent,
            lines_from_bottom,
            prompt_marker: prompt_marker.map(str::to_owned),
            screen_rules: ordered(screen_rules)?,
            title_rules: ordered(title_rules)?,
        })
    }

    /// The built-in manifest for `agent`.
    pub fn builtin(agent: AgentKind) -> Result<Self, ClassifyError> {
        crate::builtin::manifest_for(agent)
    }

    pub fn agent(&self) -> AgentKind {
        self.agent
    }

    pub fn lines_from_bottom(&self) -> usize {
        self.lines_from_bottom
    }

    pub fn prompt_marker(&self) -> Option<&str> {
        self.prompt_marker.as_deref()
    }

    pub fn screen_rules(&self) -> &[Rule] {
        &self.screen_rules
    }

    pub fn title_rules(&self) -> &[Rule] {
        &self.title_rules
    }
}

fn ordered(mut rules: Vec<Rule>) -> Result<Vec<Rule>, ClassifyError> {
    rules.sort_by_key(Rule::priority);
    let mut ids = HashSet::new();
    let mut priorities = HashSet::new();
    for r in &rules {
        if !ids.insert(r.id().as_str().to_owned()) {
            return Err(ClassifyError::DuplicateId(r.id().to_string()));
        }
        if !priorities.insert(r.priority()) {
            return Err(ClassifyError::DuplicatePriority(r.priority()));
        }
    }
    Ok(rules)
}
