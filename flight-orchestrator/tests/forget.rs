// SPDX-License-Identifier: MIT

//! Forgetting is the operator's explicit decision; disconnection alone never removes a node.

mod support;

use flight_orchestrator::{ForgetError, UiId};
use flight_proto::NodeStatusCode;
use flight_state::HostId;
use support::*;

fn world() -> World {
    let mut w = simple_world();
    w.subscribe(UiId(1));
    w.connect(0);
    w.connect(1);
    w.observe(0, round(1, vec![obs("%1", 1, PERMIT_SCREEN)]));
    w.observe(1, round(1, vec![obs("%1", 2, BUSY_SCREEN)]));
    w
}

fn forget(w: &mut World, node: &str) -> Result<(), ForgetError> {
    let fx = w.orch.forget_node(&HostId::new(node))?;
    w.deliver(fx);
    Ok(())
}

#[test]
fn a_disconnected_node_stays_known_until_it_is_forgotten() {
    let mut w = world();
    w.disconnect(0);
    for _ in 0..10 {
        w.advance(5);
    }
    assert_eq!(w.views().len(), 2, "being gone does not remove a node");
    assert_eq!(
        w.node_view("node-a").expect("a").status,
        NodeStatusCode::Disconnected as i32
    );

    forget(&mut w, "node-a").expect("forget a disconnected node");
    assert!(w.node_view("node-a").is_none());
    assert_eq!(w.views().len(), 1);
    assert_eq!(
        w.uis[&UiId(1)].views(),
        w.views(),
        "every UI saw the removal"
    );
}

#[test]
fn a_connected_or_stale_node_cannot_be_forgotten() {
    let mut w = world();
    assert_eq!(forget(&mut w, "node-a"), Err(ForgetError::StillConnected));
    w.mute(0);
    w.advance(16);
    assert_eq!(
        w.node_view("node-a").expect("a").status,
        NodeStatusCode::Stale as i32
    );
    assert_eq!(
        forget(&mut w, "node-a"),
        Err(ForgetError::StillConnected),
        "stale still has a connection"
    );
    assert_eq!(w.views().len(), 2);
}

#[test]
fn an_unknown_node_is_reported_not_ignored() {
    let mut w = world();
    assert_eq!(forget(&mut w, "node-zzz"), Err(ForgetError::Unknown));
    w.disconnect(1);
    forget(&mut w, "node-b").expect("first forget");
    assert_eq!(
        forget(&mut w, "node-b"),
        Err(ForgetError::Unknown),
        "forgetting twice"
    );
}

#[test]
fn a_forgotten_node_that_connects_again_comes_back_fresh_and_the_ui_agrees() {
    let mut w = world();
    w.disconnect(0);
    forget(&mut w, "node-a").expect("forget");
    w.nodes[0].restart("mini-1");
    w.connect(0);
    w.observe(0, round(5, vec![obs("%7", 9, BUSY_SCREEN)]));
    let view = w.node_view("node-a").expect("a is back");
    assert_eq!(view.status, NodeStatusCode::Online as i32);
    assert_eq!(
        pane_ids(&view),
        vec!["%7".to_owned()],
        "no trace of the old image"
    );
    assert_eq!(w.uis[&UiId(1)].views(), w.views());
}

#[test]
fn forgetting_one_node_leaves_the_others_untouched() {
    let mut w = world();
    let b_before = w.node_view("node-b").expect("b");
    w.disconnect(0);
    forget(&mut w, "node-a").expect("forget");
    assert_eq!(w.node_view("node-b").expect("b"), b_before);
}
