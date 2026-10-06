// SPDX-License-Identifier: MIT

use crate::{Classification, Manifest, Observation};

/// Classify the pane title against the manifest's title rules (first match by priority
/// wins). There is no prompt-marker fallback: a title that matches nothing is `None`.
pub fn classify_title(obs: &Observation, manifest: &Manifest) -> Option<Classification> {
    manifest
        .title_rules()
        .iter()
        .find(|r| r.is_match(&obs.title))
        .map(|r| Classification {
            state: r.state(),
            rule_id: r.id().clone(),
        })
}
