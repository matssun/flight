// SPDX-License-Identifier: MIT

use crate::plan::Action;
use crate::reconcile::plan;
use crate::{ConfigKey, Executor, ObservedWorkspace};
use crate::{
    Document, HostView, Observer, Origin, Outcome, Profile, RecoveryPolicy, RecoveryReport,
    RootProbe, RootSpec, SurfaceKind, SurfaceSpec, WorkspaceDefinition,
};
use std::collections::HashMap;

fn observe_all(profile: &Profile, observer: &dyn Observer) -> HashMap<String, HostView> {
    let mut views = HashMap::new();
    for w in &profile.workspaces {
        views
            .entry(w.host.clone())
            .or_insert_with(|| observer.observe(&w.host));
    }
    views
}

/// One recovery pass over the active profile: observe, plan, act, and report. Repeatable: a pass
/// that is interrupted, or run twice, converges instead of duplicating, because
///
/// - every action is planned from a fresh observation, and re-planned from another one right
///   before it is executed, so a workspace that appeared in between is bound, not started again;
/// - a start whose outcome is unknown is not retried in the pass; the next pass finds the mark
///   the executor wrote, or finds nothing and starts it once;
/// - the only writes to the saved document are bindings and records, which are idempotent.
///
/// The caller saves the document when `changed` is set.
pub fn recover(
    doc: &mut Document,
    observer: &dyn Observer,
    probe: &dyn RootProbe,
    executor: &mut dyn Executor,
    policy: &RecoveryPolicy,
) -> RecoveryReport {
    let Some(profile) = doc.active_mut() else {
        return RecoveryReport::default();
    };
    let first = plan(profile, &observe_all(profile, observer), probe, policy);
    let mut report = RecoveryReport {
        items: first.items.clone(),
        ..RecoveryReport::default()
    };
    for item in &first.items {
        for action in &item.actions {
            // Re-plan this one workspace against a fresh view just before acting on it.
            let fresh = fresh_actions(profile, &item.key, observer, probe, policy);
            if !fresh.contains(action) && !matches!(action, Action::Bind { .. }) {
                continue;
            }
            let outcome = execute(profile, executor, action, &mut report.changed);
            let unknown = matches!(outcome, Outcome::Unknown(_));
            report
                .done
                .push((item.key.clone(), action.clone(), outcome));
            if unknown {
                break;
            }
        }
    }
    report.changed |= record_unsaved(profile, &first.unsaved, observer, &mut report.recorded);
    report
}

fn fresh_actions(
    profile: &Profile,
    key: &ConfigKey,
    observer: &dyn Observer,
    probe: &dyn RootProbe,
    policy: &RecoveryPolicy,
) -> Vec<Action> {
    let mut only = Profile::new(profile.name.clone());
    only.workspaces = profile.workspaces.clone();
    only.dismissed = profile.dismissed.clone();
    plan(&only, &observe_all(&only, observer), probe, policy)
        .items
        .into_iter()
        .find(|i| &i.key == key)
        .map(|i| i.actions)
        .unwrap_or_default()
}

fn execute(
    profile: &mut Profile,
    executor: &mut dyn Executor,
    action: &Action,
    changed: &mut bool,
) -> Outcome {
    match action {
        Action::Bind { key, workspace_id } => {
            if let Some(w) = profile.get_mut(key) {
                w.last_workspace_id = Some(workspace_id.clone());
                *changed = true;
            }
            Outcome::Bound
        }
        Action::StartWorkspace { key } => {
            let Some(def) = profile.get(key).cloned() else {
                return Outcome::Refused("no longer saved".to_owned());
            };
            match executor.start_workspace(&def) {
                Ok(started) => {
                    if let Some(w) = profile.get_mut(key) {
                        w.last_workspace_id = Some(started.workspace_id);
                        *changed = true;
                    }
                    Outcome::Started
                }
                Err(e) => RecoveryReport::refused(e),
            }
        }
        Action::StartSurface {
            key,
            surface,
            kind,
            workspace_id,
        } => with_def(profile, key, |def| {
            executor
                .start_surface(def, surface, *kind, workspace_id)
                .map_or_else(RecoveryReport::refused, |()| Outcome::Started)
        }),
        Action::ResumeAgent {
            key,
            surface,
            workspace_id,
        } => with_def(profile, key, |def| {
            executor
                .resume_agent(def, surface, workspace_id)
                .map_or_else(RecoveryReport::refused, |()| Outcome::Resumed)
        }),
    }
}

fn with_def(
    profile: &Profile,
    key: &ConfigKey,
    f: impl FnOnce(&WorkspaceDefinition) -> Outcome,
) -> Outcome {
    match profile.get(key) {
        Some(def) => f(def),
        None => Outcome::Refused("no longer saved".to_owned()),
    }
}

/// Remember running workspaces nobody saved, so what the user built by hand is not lost with
/// the next restart. Dismissed ones stay dismissed.
fn record_unsaved(
    profile: &mut Profile,
    unsaved: &[String],
    observer: &dyn Observer,
    recorded: &mut Vec<String>,
) -> bool {
    let mut hosts: Vec<String> = profile.workspaces.iter().map(|w| w.host.clone()).collect();
    hosts.sort();
    hosts.dedup();
    let mut changed = false;
    for host in hosts {
        let HostView::Reachable(list) = observer.observe(&host) else {
            continue;
        };
        for o in list.iter().filter(|o| unsaved.contains(&o.workspace_id)) {
            if let Some(def) = definition_of(&host, o) {
                profile.upsert(def);
                recorded.push(o.workspace_id.clone());
                changed = true;
            }
        }
    }
    changed
}

fn definition_of(host: &str, o: &ObservedWorkspace) -> Option<WorkspaceDefinition> {
    let surfaces = o
        .surfaces
        .iter()
        .map(|s| {
            Some(SurfaceSpec {
                key: s
                    .config_key
                    .clone()
                    .map_or_else(|| ConfigKey::mint().ok(), Some)?,
                kind: s.kind,
                provider: (s.kind == SurfaceKind::Agent).then(|| "claude".to_owned()),
                skip_permissions: false,
                last_surface_id: Some(s.surface_id.clone()),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(WorkspaceDefinition {
        key: o
            .config_key
            .clone()
            .map_or_else(|| ConfigKey::mint().ok(), Some)?,
        name: o
            .root
            .rsplit('/')
            .find(|p| !p.is_empty())
            .unwrap_or("workspace")
            .to_owned(),
        host: host.to_owned(),
        root: RootSpec::new(o.root.clone()),
        surfaces,
        origin: Origin::Local,
        last_workspace_id: Some(o.workspace_id.clone()),
    })
}
