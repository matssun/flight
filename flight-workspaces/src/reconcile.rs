// SPDX-License-Identifier: MIT

use crate::matching::{assign, Match};
use crate::plan::{Action, Blocker, Health, Item, Plan, Refusal};
use crate::{
    HostView, ObservedWorkspace, Origin, Profile, RecoveryPolicy, RootCheck, RootProbe, RootState,
    SurfaceKind, SurfaceSpec, WorkspaceDefinition,
};
use std::collections::HashMap;

/// Compare what is saved with what is running and say what may be done about it. Pure apart from
/// the read-only probe: nothing is executed here, and the same inputs give the same plan.
pub fn plan(
    profile: &Profile,
    views: &HashMap<String, HostView>,
    probe: &dyn RootProbe,
    policy: &RecoveryPolicy,
) -> Plan {
    let mut plan = Plan::default();
    let mut hosts: Vec<&str> = profile.workspaces.iter().map(|w| w.host.as_str()).collect();
    hosts.extend(views.keys().map(String::as_str));
    hosts.sort_unstable();
    hosts.dedup();
    let mut accounted: Vec<String> = Vec::new();
    for host in hosts {
        let defs: Vec<&WorkspaceDefinition> = profile
            .workspaces
            .iter()
            .filter(|w| w.host == host)
            .collect();
        let (seen, reason): (Vec<&ObservedWorkspace>, Option<&str>) = match views.get(host) {
            Some(HostView::Reachable(list)) => (list.iter().collect(), None),
            Some(HostView::Unreachable { reason }) => (Vec::new(), Some(reason.as_str())),
            None => (Vec::new(), Some("the host was not asked")),
        };
        let matches = assign(&defs, &seen);
        for (def, found) in defs.iter().zip(matches) {
            let state = match reason {
                Some(r) => RootState::HostUnreachable {
                    reason: r.to_owned(),
                },
                None => probe.probe(host, &def.root.path),
            };
            let root = def.root.check(&state);
            let observed = match &found {
                Match::One(i) => seen.get(*i).copied(),
                _ => None,
            };
            if let Some(o) = observed {
                accounted.push(o.workspace_id.clone());
            }
            plan.items
                .push(item(def, &found, observed, reason, root, policy));
        }
        plan.unsaved.extend(
            seen.iter()
                .map(|o| o.workspace_id.clone())
                .filter(|id| !accounted.contains(id) && !profile.dismissed.contains(id))
                .filter(|id| !plan.items.iter().any(|i| ambiguous_with(i, id))),
        );
    }
    plan
}

fn ambiguous_with(item: &Item, id: &str) -> bool {
    matches!(&item.health, Health::Blocked(Blocker::Ambiguous(c)) if c.iter().any(|c| c == id))
}

fn item(
    def: &WorkspaceDefinition,
    found: &Match,
    observed: Option<&ObservedWorkspace>,
    unreachable: Option<&str>,
    root: RootCheck,
    policy: &RecoveryPolicy,
) -> Item {
    let mut it = Item {
        key: def.key.clone(),
        health: Health::Stopped,
        root: root.clone(),
        actions: Vec::new(),
        refusals: Vec::new(),
    };
    if let Some(reason) = unreachable {
        it.health = Health::Blocked(Blocker::HostUnreachable(reason.to_owned()));
        return it;
    }
    if let Match::Ambiguous(ids) = found {
        it.health = Health::Blocked(Blocker::Ambiguous(ids.clone()));
        return it;
    }
    let Some(live) = observed else {
        if root.usable() {
            let any_skip = def.surfaces.iter().any(|s| s.skip_permissions);
            match gate(def, any_skip, &root, policy) {
                Ok(()) => it.actions.push(Action::StartWorkspace {
                    key: def.key.clone(),
                }),
                Err(r) => it.refusals.push(r),
            }
        } else {
            it.health = Health::Blocked(Blocker::Root(root));
        }
        return it;
    };
    if def.last_workspace_id.as_deref() != Some(&live.workspace_id) {
        it.actions.push(Action::Bind {
            key: def.key.clone(),
            workspace_id: live.workspace_id.clone(),
        });
    }
    let missing = missing_surfaces(def, live);
    it.health = if missing.is_empty() {
        Health::Running
    } else {
        Health::Partial
    };
    for spec in missing {
        match gate(def, spec.skip_permissions, &root, policy) {
            Ok(()) => it.actions.push(start_or_resume(def, spec, live, policy)),
            Err(r) => it.refusals.push(r),
        }
    }
    it
}

fn start_or_resume(
    def: &WorkspaceDefinition,
    spec: &SurfaceSpec,
    live: &ObservedWorkspace,
    policy: &RecoveryPolicy,
) -> Action {
    let resumable = spec.kind == SurfaceKind::Agent
        && spec
            .provider
            .as_ref()
            .is_some_and(|p| policy.resumable_providers.contains(p));
    if resumable {
        Action::ResumeAgent {
            key: def.key.clone(),
            surface: spec.key.clone(),
            workspace_id: live.workspace_id.clone(),
        }
    } else {
        Action::StartSurface {
            key: def.key.clone(),
            surface: spec.key.clone(),
            kind: spec.kind,
            workspace_id: live.workspace_id.clone(),
        }
    }
}

/// Saved surfaces with no running counterpart: by the key Flight wrote, by the runtime id last
/// seen, or (an unmarked, older workspace) by kind.
fn missing_surfaces<'a>(
    def: &'a WorkspaceDefinition,
    live: &ObservedWorkspace,
) -> Vec<&'a SurfaceSpec> {
    let mut used: Vec<usize> = Vec::new();
    let mut missing = Vec::new();
    for spec in &def.surfaces {
        let by = |test: &dyn Fn(&crate::ObservedSurface) -> bool| {
            live.surfaces
                .iter()
                .enumerate()
                .find(|(i, o)| !used.contains(i) && test(o))
                .map(|(i, _)| i)
        };
        let hit = by(&|o| o.config_key.as_ref() == Some(&spec.key))
            .or_else(|| {
                by(&|o| {
                    o.config_key.is_none() && spec.last_surface_id.as_deref() == Some(&o.surface_id)
                })
            })
            .or_else(|| by(&|o| o.config_key.is_none() && o.kind == spec.kind));
        match hit {
            Some(i) => used.push(i),
            None => missing.push(spec),
        }
    }
    missing
}

fn gate(
    def: &WorkspaceDefinition,
    skip_permissions: bool,
    root: &RootCheck,
    policy: &RecoveryPolicy,
) -> Result<(), Refusal> {
    if !policy.start_missing {
        return Err(Refusal::PolicyDoesNotStart);
    }
    if def.origin == Origin::Imported && !policy.start_imported {
        return Err(Refusal::ImportedNotTrusted);
    }
    if skip_permissions && !policy.start_skip_permissions {
        return Err(Refusal::SkipPermissionsNotTrusted);
    }
    if !root.usable() {
        return Err(Refusal::RootNotVerified);
    }
    Ok(())
}
