// SPDX-License-Identifier: MIT

//! Bounded queues and off-lock control work over the real transport.

mod support;

use flight_node::{Control, ControlError, SessionRequest};
use flight_proto::{
    command_kind as ck, fleet_change::Change, response_result, ui_event_body, ui_request_body,
    Command, ErrorKindCode, NodeStatusCode, ReplicationCursor, Request, Response, StateCode,
    Subscribe, UiRequest,
};
use flight_state::{PaneId, ServerId};
use flight_transport::{UiClient, OUTBOX_CAPACITY};
use flight_trust::Identity;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;
use tokio::sync::watch;

fn subscribe() -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
    }
}

fn preview(node: &str, id: u64) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command {
                kind: Some(ck::Kind::GetPreview(ck::GetPreview {
                    pane_ref: Some(flight_proto::PaneRefMsg {
                        host: node.into(),
                        server: "flight".into(),
                        pane: "%1".into(),
                    }),
                    lines: 10,
                })),
            }),
        })),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_that_stops_reading_cannot_make_the_orchestrator_buffer_without_bound() {
    let node = Arc::new(Identity::generate().expect("node"));
    let ui_id = Identity::generate().expect("ui");
    let (server, orch, addr) = start(
        trust_with(&[(&node, "mini-1")], &[(&ui_id, "laptop")]),
        None,
    )
    .await;
    let fp = node.fingerprint().clone();
    let link = node_link(&node, &addr, &orch, "mini-1", None, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("online", || {
        node_status(&server, &fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    link.observe(vec![round(1, vec![obs("%1", 7, PERMIT_SCREEN)])]);
    wait_until("pane", || pane_count(&server, &fp) == 1).await;

    // A UI subscribes and then never reads.
    let mut ui = UiClient::connect(&addr, &ui_id, &orch).await.expect("ui");
    ui.send(subscribe()).expect("send");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A flood of state changes: every observation flips the pane's state.
    let mut worst = 0;
    for i in 0..20_000u64 {
        let screen = if i % 2 == 0 {
            BUSY_SCREEN
        } else {
            PERMIT_SCREEN
        };
        link.observe(vec![round(10 + i, vec![obs("%1", 7, screen)])]);
        if i % 500 == 0 {
            tokio::task::yield_now().await;
            worst = worst.max(server.max_backlog());
        }
    }
    worst = worst.max(server.max_backlog());
    assert!(
        worst <= OUTBOX_CAPACITY,
        "backlog {worst} exceeded the bound {OUTBOX_CAPACITY}"
    );

    // When the UI reads again it is resynchronized with a snapshot and converges on the truth.
    let mut cursor = ReplicationCursor::new();
    let mut snapshots = 0;
    let mut last_state = None;
    let drain = async {
        loop {
            let Some(event) = ui.next_event().await.expect("stream") else {
                return;
            };
            match event.body {
                Some(ui_event_body::Body::Snapshot(s)) => {
                    snapshots += 1;
                    cursor.on_snapshot(s.incarnation().expect("inc"));
                    last_state = s
                        .nodes
                        .first()
                        .and_then(|n| n.panes.first())
                        .map(|p| p.state);
                }
                Some(ui_event_body::Body::Delta(d)) => {
                    assert_eq!(
                        cursor.on_delta(d.incarnation().expect("inc"), d.sequence),
                        flight_proto::Step::Apply,
                        "deltas after a snapshot stay in sequence"
                    );
                    if let Some(Change::PaneUpsert(p)) = d.change {
                        last_state = Some(p.state);
                    }
                }
                _ => {}
            }
            let truth = server.fleet_snapshot().nodes[0].panes[0].state;
            if last_state == Some(truth) && snapshots >= 2 {
                return;
            }
        }
    };
    within("ui converges", drain).await;
    assert!(
        snapshots >= 2,
        "a resync snapshot must have been sent after the overflow"
    );

    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    server.shutdown().await;
}

/// A control whose operations take a while: it must not hold anything up.
struct SlowControl(Duration);

impl Control for SlowControl {
    fn capture(&self, _: &ServerId, _: &PaneId, _: u32) -> Result<String, ControlError> {
        std::thread::sleep(self.0);
        Ok("slow preview".into())
    }
    fn kill_pane(&self, _: &ServerId, _: &PaneId, _: u32) -> Result<(), ControlError> {
        Ok(())
    }
    fn create_session(&self, _: &SessionRequest) -> Result<(), ControlError> {
        Ok(())
    }
}

async fn slow_rig(
    delay: Duration,
) -> (
    flight_transport::ServerHandle,
    UiClient,
    Arc<flight_transport::NodeLink>,
    flight_trust::Fingerprint,
    watch::Sender<bool>,
    tokio::task::JoinHandle<flight_transport::LinkEnd>,
) {
    let node = Arc::new(Identity::generate().expect("node"));
    let ui_id = Identity::generate().expect("ui");
    let (server, orch, addr) = start(
        trust_with(&[(&node, "mini-1")], &[(&ui_id, "laptop")]),
        None,
    )
    .await;
    let fp = node.fingerprint().clone();
    let link = node_link_with(
        &node,
        &addr,
        &orch,
        "mini-1",
        None,
        1,
        Arc::new(SlowControl(delay)),
    );
    let (stop_tx, stop_rx) = watch::channel(false);
    let runner = {
        let link = link.clone();
        tokio::spawn(async move { link.run(stop_rx).await })
    };
    wait_until("online", || {
        node_status(&server, &fp) == Some(NodeStatusCode::Online as i32)
    })
    .await;
    link.observe(vec![round(1, vec![obs("%1", 7, PERMIT_SCREEN)])]);
    wait_until("pane", || pane_count(&server, &fp) == 1).await;
    let mut ui = UiClient::connect(&addr, &ui_id, &orch).await.expect("ui");
    ui.send(subscribe()).expect("send");
    ui_until(&mut ui, "snapshot", |e| {
        matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
    })
    .await;
    (server, ui, link, fp, stop_tx, runner)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_tmux_call_does_not_stall_replication_or_other_requests() {
    let (server, mut ui, link, fp, stop_tx, runner) = slow_rig(Duration::from_millis(1500)).await;
    let started = Instant::now();
    ui.send(preview(fp.as_str(), 1)).expect("send");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // While the capture is stuck in tmux, the node still replicates state...
    link.observe(vec![round(2, vec![obs("%1", 7, BUSY_SCREEN)])]);
    ui_until(&mut ui, "delta while a preview is pending", |e| {
        matches!(&e.body, Some(ui_event_body::Body::Delta(d)) if matches!(&d.change,
            Some(Change::PaneUpsert(p)) if p.state == StateCode::Busy as i32))
    })
    .await;
    assert!(
        started.elapsed() < Duration::from_millis(1400),
        "replication waited for tmux"
    );

    // ...and the preview does complete afterwards.
    let answer = ui_until(&mut ui, "preview", |e| {
        matches!(e.body, Some(ui_event_body::Body::Response(_)))
    })
    .await;
    assert!(matches!(
        answer.body,
        Some(ui_event_body::Body::Response(Response {
            request_id: 1,
            result: Some(response_result::Result::Preview(_))
        }))
    ));
    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    server.shutdown().await;
    drop(fp);
}

#[tokio::test(flavor = "multi_thread")]
async fn control_work_is_bounded_and_excess_requests_are_refused_at_once() {
    let (server, mut ui, _link, fp, stop_tx, runner) = slow_rig(Duration::from_millis(1500)).await;
    for id in 1..=8u64 {
        ui.send(preview(fp.as_str(), id)).expect("send");
    }
    // Four workers; the rest are answered "busy" without waiting for any tmux call.
    let mut busy = 0;
    let started = Instant::now();
    while busy < 4 {
        let event = ui_until(&mut ui, "busy refusal", |e| {
            matches!(e.body, Some(ui_event_body::Body::Response(_)))
        })
        .await;
        if let Some(ui_event_body::Body::Response(Response {
            result: Some(response_result::Result::Error(e)),
            ..
        })) = event.body
        {
            assert_eq!(e.kind, ErrorKindCode::RemoteCommandFailed as i32);
            busy += 1;
        }
    }
    assert!(
        started.elapsed() < Duration::from_millis(1200),
        "refusals must not wait for tmux"
    );
    stop_tx.send(true).expect("stop");
    runner.await.expect("runner");
    server.shutdown().await;
}
