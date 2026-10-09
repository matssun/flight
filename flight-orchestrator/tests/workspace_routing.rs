// SPDX-License-Identifier: MIT

//! Workspace surfaces through the orchestrator: a `CreateSurface` names a workspace and
//! nothing else, and is routed to the host that workspace lives on; the fleet a UI sees is
//! rebuilt from what nodes publish, after any restart.

mod support;

use flight_node::PaneObservation;
use flight_orchestrator::UiId;
use flight_proto::{
    command_kind as ck, orchestrator_body, response_result, ui_event_body, ui_request_body,
    Command, ErrorKindCode, PaneState, Request, Response, SurfaceKindCode, UiRequest,
};
use flight_state::RawPlacement;
use support::*;

/// A pane of workspace `ws`: surface `surface` of kind `kind` (`agent` or `shell`).
fn surface_obs(pane: &str, pid: u32, ws: &str, surface: &str, kind: &str) -> PaneObservation {
    let mut o = obs(pane, pid, IDLE_SCREEN);
    o.placement = RawPlacement {
        workspace_id: ws.into(),
        surface_id: surface.into(),
        surface_kind: kind.into(),
        window_id: "@1".into(),
        session_id: "$1".into(),
        session_path: "/work/nga".into(),
    };
    o
}

fn create_surface(id: u64, ws: &str) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::CreateSurface(ck::CreateSurface {
                    workspace_id: ws.into(),
                    kind: SurfaceKindCode::Shell as i32,
                })),
            }),
        })),
    }
}

fn error_kinds(w: &World) -> Vec<i32> {
    w.responses
        .iter()
        .filter_map(|(_, e)| match &e.body {
            Some(ui_event_body::Body::Response(Response {
                result: Some(response_result::Result::Error(e)),
                ..
            })) => Some(e.kind),
            _ => None,
        })
        .collect()
}

fn send(w: &mut World, req: UiRequest) {
    let fx = w.orch.ui_request(UiId(1), req, w.now);
    w.deliver(fx);
}

/// Node A has workspace `w-a`, node B has `w-b`; both online.
fn two_workspaces() -> World {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(
        0,
        round(1, vec![surface_obs("%1", 10, "w-a", "s-a1", "agent")]),
    );
    w.observe(
        1,
        round(1, vec![surface_obs("%1", 20, "w-b", "s-b1", "agent")]),
    );
    w
}

fn panes_of(w: &World, node: &str) -> Vec<PaneState> {
    let mut panes: Vec<PaneState> = w.uis[&UiId(1)]
        .nodes()
        .get(node)
        .map(|n| n.panes.values().cloned().collect())
        .unwrap_or_default();
    panes.sort_by(|a, b| a.surface_id.cmp(&b.surface_id));
    panes
}

#[test]
fn a_create_surface_goes_to_the_host_that_has_the_workspace() {
    let mut w = two_workspaces();
    send(&mut w, create_surface(1, "w-b"));
    assert_eq!(w.forwarded.len(), 1);
    assert_eq!(w.forwarded[0].0, "node-b", "routed by the workspace's host");
    let Some(orchestrator_body::Body::Request(forwarded)) = w.forwarded[0].1.body.clone() else {
        panic!("not a request")
    };
    // What the node gets names the workspace and nothing else.
    let Some(Command {
        kind: Some(ck::Kind::CreateSurface(c)),
    }) = forwarded.command
    else {
        panic!("not a create_surface")
    };
    assert_eq!(c.workspace_id, "w-b");
}

#[test]
fn the_other_hosts_workspace_is_not_reachable_through_the_first() {
    let mut w = two_workspaces();
    send(&mut w, create_surface(1, "w-a"));
    assert_eq!(w.forwarded.len(), 1);
    assert_eq!(w.forwarded[0].0, "node-a");
}

#[test]
fn an_unknown_workspace_is_a_typed_error_and_nothing_is_forwarded() {
    let mut w = two_workspaces();
    send(&mut w, create_surface(1, "w-nope"));
    assert_eq!(
        error_kinds(&w),
        vec![ErrorKindCode::UnknownWorkspace as i32]
    );
    assert!(w.forwarded.is_empty());
}

#[test]
fn a_workspace_id_two_hosts_claim_is_not_routed_on_a_guess() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(
        0,
        round(1, vec![surface_obs("%1", 10, "w-same", "s-1", "agent")]),
    );
    w.observe(
        1,
        round(1, vec![surface_obs("%1", 20, "w-same", "s-2", "agent")]),
    );
    send(&mut w, create_surface(1, "w-same"));
    assert_eq!(
        error_kinds(&w),
        vec![ErrorKindCode::UnknownWorkspace as i32]
    );
    assert!(w.forwarded.is_empty());
}

#[test]
fn a_workspace_on_a_disconnected_node_fails_at_once_and_is_not_queued() {
    let mut w = two_workspaces();
    w.disconnect(1);
    send(&mut w, create_surface(1, "w-b"));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::NodeUnreachable as i32]);
    assert!(w.forwarded.is_empty());
    w.connect(1);
    assert!(w.forwarded.is_empty(), "never replayed");
}

#[test]
fn a_node_that_cannot_make_surfaces_is_told_nothing() {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    // An older build: it offers terminals and sessions but not surfaces.
    w.connect_offering(0, &["preview", "create_session_v1"]);
    w.observe(
        0,
        round(1, vec![surface_obs("%1", 10, "w-a", "s-a1", "agent")]),
    );
    send(&mut w, create_surface(1, "w-a"));
    assert_eq!(error_kinds(&w), vec![ErrorKindCode::Unsupported as i32]);
    assert!(w.forwarded.is_empty());
}

#[test]
fn a_ui_that_sends_a_malformed_workspace_is_refused_before_routing() {
    let mut w = two_workspaces();
    for bad in ["", "a b", "$3", "w:1"] {
        send(&mut w, create_surface(2, bad));
    }
    assert_eq!(
        error_kinds(&w),
        vec![ErrorKindCode::InvalidRequest as i32; 4]
    );
    assert!(w.forwarded.is_empty());
}

#[test]
fn surfaces_of_a_workspace_arrive_as_they_are_published_and_leave_with_the_window() {
    let mut w = two_workspaces();
    w.observe(
        0,
        round(
            2,
            vec![
                surface_obs("%1", 10, "w-a", "s-a1", "agent"),
                surface_obs("%2", 11, "w-a", "s-a2", "shell"),
            ],
        ),
    );
    let panes = panes_of(&w, "node-a");
    assert_eq!(panes.len(), 2);
    assert!(panes.iter().all(|p| p.workspace_id == "w-a"));
    let kinds: Vec<i32> = panes.iter().map(|p| p.surface_kind).collect();
    assert_eq!(
        kinds,
        vec![SurfaceKindCode::Agent as i32, SurfaceKindCode::Shell as i32]
    );
    // The shell's window goes away: one surface is left, the workspace is not.
    w.observe(
        0,
        round(3, vec![surface_obs("%1", 10, "w-a", "s-a1", "agent")]),
    );
    assert_eq!(panes_of(&w, "node-a").len(), 1);
}

#[test]
fn the_orchestrator_restarting_rebuilds_every_workspace_and_surface_from_the_nodes() {
    let mut w = two_workspaces();
    w.observe(
        0,
        round(
            2,
            vec![
                surface_obs("%1", 10, "w-a", "s-a1", "agent"),
                surface_obs("%2", 11, "w-a", "s-a2", "shell"),
            ],
        ),
    );
    let before = (panes_of(&w, "node-a"), panes_of(&w, "node-b"));
    w.restart_orchestrator();
    assert!(
        panes_of(&w, "node-a").is_empty(),
        "nothing is kept centrally"
    );
    w.connect(0);
    w.connect(1);
    let after = (panes_of(&w, "node-a"), panes_of(&w, "node-b"));
    assert_eq!(before, after);
    // And the rebuilt fleet routes again.
    send(&mut w, create_surface(9, "w-b"));
    assert_eq!(w.forwarded.last().map(|f| f.0.as_str()), Some("node-b"));
}

#[test]
fn a_node_restarting_over_a_live_backend_republishes_the_same_ids() {
    let mut w = two_workspaces();
    let before = panes_of(&w, "node-a");
    w.disconnect(0);
    w.nodes[0].restart("mini-1");
    w.connect(0);
    // The backend still holds the windows (and their markers): the same observation again.
    w.observe(
        0,
        round(5, vec![surface_obs("%1", 10, "w-a", "s-a1", "agent")]),
    );
    let after = panes_of(&w, "node-a");
    assert_eq!(
        before
            .iter()
            .map(|p| (&p.workspace_id, &p.surface_id))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|p| (&p.workspace_id, &p.surface_id))
            .collect::<Vec<_>>()
    );
}
