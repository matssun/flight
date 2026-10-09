// SPDX-License-Identifier: MIT

use crate::{ObservedWorkspace, WorkspaceDefinition};
use std::collections::HashSet;

/// Which running workspace, if any, realizes a saved one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Match {
    None,
    One(usize),
    /// Several could be it; the ids of the candidates.
    Ambiguous(Vec<String>),
}

fn same_root(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Match definitions to the running workspaces of their host, strongest evidence first: the key
/// Flight wrote into the backend when it started the workspace, then the runtime id last seen,
/// then an unmarked workspace in the same root. A weaker tier only considers what a stronger
/// one left unclaimed, and a tie is `Ambiguous`, never a guess.
pub(crate) fn assign(defs: &[&WorkspaceDefinition], seen: &[&ObservedWorkspace]) -> Vec<Match> {
    let mut out = vec![Match::None; defs.len()];
    let mut claimed: HashSet<usize> = HashSet::new();
    type Test = dyn Fn(&WorkspaceDefinition, &ObservedWorkspace) -> bool;
    let tiers: [&Test; 3] = [
        &|d, o| o.config_key.as_ref() == Some(&d.key),
        &|d, o| o.config_key.is_none() && d.last_workspace_id.as_deref() == Some(&o.workspace_id),
        &|d, o| o.config_key.is_none() && same_root(&d.root.path, &o.root),
    ];
    for test in tiers {
        let mut round: Vec<(usize, Vec<usize>)> = Vec::new();
        for (di, def) in defs.iter().enumerate() {
            if !matches!(out.get(di), Some(Match::None)) {
                continue;
            }
            let hits: Vec<usize> = seen
                .iter()
                .enumerate()
                .filter(|(oi, o)| !claimed.contains(oi) && test(def, o))
                .map(|(oi, _)| oi)
                .collect();
            if !hits.is_empty() {
                round.push((di, hits));
            }
        }
        // Two definitions that both want the same workspace make both ambiguous.
        let mut wanted: Vec<usize> = round.iter().flat_map(|(_, h)| h.clone()).collect();
        wanted.sort_unstable();
        for (di, hits) in &round {
            let contested = hits
                .iter()
                .any(|h| wanted.iter().filter(|w| *w == h).count() > 1);
            let result = match (hits.as_slice(), contested) {
                ([only], false) => Match::One(*only),
                _ => Match::Ambiguous(
                    hits.iter()
                        .filter_map(|h| seen.get(*h))
                        .map(|o| o.workspace_id.clone())
                        .collect(),
                ),
            };
            if let (Match::One(oi), Some(slot)) = (&result, out.get_mut(*di)) {
                claimed.insert(*oi);
                *slot = result.clone();
            } else if let Some(slot) = out.get_mut(*di) {
                *slot = result;
            }
        }
    }
    out
}
