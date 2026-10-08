// SPDX-License-Identifier: MIT

#![allow(dead_code)]

use flight_classify::AgentKind;
use flight_node::{
    Control, ControlError, NodeCore, NodeSession, PaneObservation, Round, ServerOutcome,
    SessionRequest,
};
use flight_orchestrator::OrchestratorConfig;
use flight_proto::{Incarnation, UiEvent};
use flight_state::{HostId, PaneId, ServerId};
use flight_transport::{serve, NodeLink, NodeLinkConfig, ServerConfig, ServerHandle, UiClient};
use flight_trust::{Fingerprint, Identity, Role, TrustStore};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const PERMIT_SCREEN: &str =
    include_str!("../../../flight-classify/tests/fixtures/claude-permit.txt");
pub const BUSY_SCREEN: &str = "✻ Trapping Gollum… (8s · ↑ 240 tokens)\n\n❯\n";

pub struct PreviewControl;

impl Control for PreviewControl {
    fn capture(&self, _: &ServerId, pane: &PaneId, lines: u32) -> Result<String, ControlError> {
        Ok(format!("preview of {pane} ({lines} lines)"))
    }
    fn kill_pane(&self, _: &ServerId, _: &PaneId, _: u32) -> Result<(), ControlError> {
        Ok(())
    }
    fn create_session(&self, _: &SessionRequest) -> Result<(), ControlError> {
        Ok(())
    }
}

pub fn inc(n: u8) -> Incarnation {
    Incarnation::from_bytes([n; Incarnation::LEN])
}

pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flight-transport-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

pub async fn start(
    trust: TrustStore,
    trust_path: Option<PathBuf>,
) -> (ServerHandle, Fingerprint, String) {
    start_with_core(trust, trust_path, OrchestratorConfig::default()).await
}

pub async fn start_with_core(
    trust: TrustStore,
    trust_path: Option<PathBuf>,
    core: OrchestratorConfig,
) -> (ServerHandle, Fingerprint, String) {
    let identity = Identity::generate().expect("orchestrator identity");
    let orchestrator = identity.fingerprint().clone();
    let handle = serve(ServerConfig {
        bind: "127.0.0.1:0".parse().expect("addr"),
        identity,
        trust,
        trust_path,
        core,
        incarnation: inc(77),
        tick_interval: Duration::from_millis(100),
    })
    .await
    .expect("serve");
    let addr = handle.local_addr().to_string();
    (handle, orchestrator, addr)
}

pub fn trust_with(nodes: &[(&Identity, &str)], uis: &[(&Identity, &str)]) -> TrustStore {
    let mut store = TrustStore::empty();
    for (id, name) in nodes {
        store.authorize(id.fingerprint(), name, Role::Node);
    }
    for (id, name) in uis {
        store.authorize(id.fingerprint(), name, Role::Ui);
    }
    store
}

/// A node link whose session claims `claimed` as its host id (normally its own fingerprint).
pub fn node_link(
    identity: &Arc<Identity>,
    addr: &str,
    orchestrator: &Fingerprint,
    name: &str,
    claimed: Option<HostId>,
    incarnation: u8,
) -> Arc<NodeLink> {
    node_link_with(
        identity,
        addr,
        orchestrator,
        name,
        claimed,
        incarnation,
        Arc::new(PreviewControl),
    )
}

pub fn node_link_with(
    identity: &Arc<Identity>,
    addr: &str,
    orchestrator: &Fingerprint,
    name: &str,
    claimed: Option<HostId>,
    incarnation: u8,
    control: Arc<dyn Control>,
) -> Arc<NodeLink> {
    let host = claimed.unwrap_or_else(|| identity.fingerprint().host_id());
    let session = NodeSession::new(NodeCore::new(host, inc(incarnation)), name);
    Arc::new(NodeLink::new(
        NodeLinkConfig {
            address: addr.to_owned(),
            identity: identity.clone(),
            orchestrator: orchestrator.clone(),
            servers: vec!["flight".to_owned()],
            heartbeat_interval: Duration::from_millis(500),
            reconnect_min: Duration::from_millis(50),
            reconnect_max: Duration::from_millis(200),
        },
        session,
        control,
    ))
}

/// A node link whose terminals give up on a far end that stays behind for `stall`.
pub fn node_link_stalling(
    identity: &Arc<Identity>,
    addr: &str,
    orchestrator: &Fingerprint,
    control: Arc<dyn Control>,
    stall: Duration,
) -> Arc<NodeLink> {
    let session = NodeSession::new(
        NodeCore::new(identity.fingerprint().host_id(), inc(1)),
        "mini-1",
    );
    Arc::new(
        NodeLink::new(
            NodeLinkConfig {
                address: addr.to_owned(),
                identity: identity.clone(),
                orchestrator: orchestrator.clone(),
                servers: vec!["flight".to_owned()],
                heartbeat_interval: Duration::from_millis(500),
                reconnect_min: Duration::from_millis(50),
                reconnect_max: Duration::from_millis(200),
            },
            session,
            control,
        )
        .with_terminal_stall(stall),
    )
}

pub fn obs(pane: &str, pid: u32, screen: &str) -> PaneObservation {
    PaneObservation {
        pane: PaneId::new(pane),
        pid,
        agent: AgentKind::Claude,
        session: "work".into(),
        window: "agent".into(),
        path: "/tmp".into(),
        command: "claude".into(),
        title: String::new(),
        focused: false,
        screen_lines: screen.lines().map(str::to_owned).collect(),
    }
}

pub fn round(now: u64, panes: Vec<PaneObservation>) -> Round {
    Round {
        server: ServerId::new("flight"),
        now,
        outcome: ServerOutcome::Observed(panes),
    }
}

/// Poll until `check` is true, or fail the test after five seconds.
pub async fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Read UI events until `pred` accepts one (returned), or fail after five seconds.
pub async fn ui_until(
    ui: &mut UiClient,
    what: &str,
    mut pred: impl FnMut(&UiEvent) -> bool,
) -> UiEvent {
    let wait = async {
        loop {
            match ui.next_event().await.expect("ui stream") {
                Some(e) if pred(&e) => return e,
                Some(_) => continue,
                None => panic!("ui stream ended while waiting for: {what}"),
            }
        }
    };
    within(what, wait).await
}

pub async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
}

pub fn node_status(handle: &ServerHandle, node: &Fingerprint) -> Option<i32> {
    handle
        .fleet_snapshot()
        .nodes
        .iter()
        .find(|n| n.node_id == node.as_str())
        .map(|n| n.status)
}

pub fn pane_count(handle: &ServerHandle, node: &Fingerprint) -> usize {
    handle
        .fleet_snapshot()
        .nodes
        .iter()
        .find(|n| n.node_id == node.as_str())
        .map_or(0, |n| n.panes.len())
}
