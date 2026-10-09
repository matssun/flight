// SPDX-License-Identifier: MIT

use crate::codes::{agent_code, source_code};
use crate::PaneObservation;
use flight_classify::{
    classify_screen, classify_title, resolve, why, working_glyph_present, AgentKind, Evidence,
    Manifest, Observation, ResolveInput, ResolvedState, DEFAULT_IDLE_SECS,
};
use flight_proto::{PaneRefMsg, PaneState, SurfaceKindCode};
use flight_state::{PaneRef, SurfaceRole};

/// Classify, fuse and resolve one observed pane against its previous resolution.
pub(crate) fn resolve_observation(
    obs: &PaneObservation,
    manifest: Option<&Manifest>,
    previous: Option<&ResolvedState>,
    now: u64,
) -> ResolvedState {
    let observation = Observation {
        screen_lines: obs.screen_lines.clone(),
        title: obs.title.clone(),
    };
    let base = Evidence {
        focused: obs.focused,
        ..Evidence::empty(obs.agent, now)
    };
    let evidence = match manifest {
        Some(m) => Evidence {
            screen: classify_screen(&observation, m),
            title: classify_title(&observation, m),
            ..base
        },
        None => base,
    };
    resolve(&ResolveInput {
        previous,
        evidence: &evidence,
        glyph_seen: working_glyph_present(&observation.screen_lines),
        idle_secs: DEFAULT_IDLE_SECS,
    })
}

/// The externally visible state for a resolved pane. `changed_at` is the time the state
/// value last changed, carried over while it stays the same.
pub(crate) fn pane_state(
    pane_ref: &PaneRef,
    obs: &PaneObservation,
    resolved: &ResolvedState,
    previous: Option<&PaneState>,
    now: u64,
) -> PaneState {
    let state = PaneState::state_code(resolved.state) as i32;
    let changed_at = match previous {
        Some(p) if p.state == state => p.changed_at,
        _ => now,
    };
    PaneState {
        pane_ref: Some(PaneRefMsg::from(pane_ref)),
        agent_kind: agent_code(obs.agent) as i32,
        state,
        source: source_code(resolved) as i32,
        rule_id: resolved
            .provenance
            .fused
            .rule_id()
            .map(ToString::to_string)
            .unwrap_or_default(),
        why: why(resolved),
        changed_at,
        session: obs.session.clone(),
        window: obs.window.clone(),
        path: obs.path.clone(),
        command: obs.command.clone(),
        pid: obs.pid,
        workspace_id: obs
            .placement
            .workspace(&pane_ref.host, &pane_ref.server)
            .to_string(),
        surface_id: obs
            .placement
            .surface(&pane_ref.host, &pane_ref.server)
            .to_string(),
        surface_kind: match obs.placement.role(obs.agent != AgentKind::Other) {
            SurfaceRole::Agent => SurfaceKindCode::Agent,
            SurfaceRole::Shell => SurfaceKindCode::Shell,
        } as i32,
        workspace_root: obs.placement.root(&obs.path).to_owned(),
    }
}
