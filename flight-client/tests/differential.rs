// SPDX-License-Identifier: MIT

//! Two ways to the same answer. The same scripted tmux server is watched by
//!   A: the direct collector (the original local/SSH path), and
//!   B: node -> orchestrator -> UiClient (the distributed path, over real mutual TLS).
//! At every step the dashboard data from B must agree with A.

use flight_client::{ClientConfig, OrchestratedBackend};
use flight_control::{BoxedRunner, HostRegistry, Transport};
use flight_node::{NodeCore, NodeSession, TmuxServers};
use flight_orchestrator::OrchestratorConfig;
use flight_proto::Incarnation;
use flight_state::{AgentState, HostId, ServerId};
use flight_tmux::{TmuxEndpoint, TmuxError, TmuxOutput, TmuxRunner};
use flight_transport::{serve, NodeLink, NodeLinkConfig, ServerConfig};
use flight_trust::{Identity, Role, TrustStore};
use flight_ui::{Backend, Collector, HostHealth, UiSnapshot};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PERMIT: &str = include_str!("../../flight-classify/tests/fixtures/claude-permit.txt");
const BUSY: &str = "✻ Trapping Gollum… (8s · ↑ 240 tokens)\n\n❯\n";
const IDLE: &str = "Done!\n\n❯\n";

#[derive(Default)]
struct Table {
    panes: String,
    screens: HashMap<String, String>,
    list_error: Option<String>,
}

/// One scripted tmux server, shared by both paths.
#[derive(Clone, Default)]
struct Script(Arc<Mutex<Table>>);

impl Script {
    fn set(&self, panes: &[(&str, &str, u32)], screens: &[(&str, &str)]) {
        let mut t = self.0.lock().unwrap();
        t.list_error = None;
        t.panes = panes
            .iter()
            .map(|(id, cmd, pid)| {
                format!("{id}\twork\tw\t@1\t0\t/tmp\t{pid}\t0\t0\t0\t{cmd}\t1700\t0\t\t\t\t/tmp\t$1\t\t\ttitle\n")
            })
            .collect();
        t.screens = screens
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
    }

    fn fail(&self, stderr: &str) {
        self.0.lock().unwrap().list_error = Some(stderr.to_owned());
    }
}

impl TmuxRunner for Script {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let t = self.0.lock().unwrap();
        if let Some(stderr) = &t.list_error {
            return Err(TmuxError::Failed {
                code: Some(1),
                stderr: stderr.clone(),
            });
        }
        match args.first().copied() {
            Some("list-panes") => Ok(TmuxOutput {
                stdout: t.panes.clone(),
            }),
            Some("capture-pane") => {
                let target = args
                    .iter()
                    .position(|a| *a == "-t")
                    .and_then(|i| args.get(i + 1))
                    .copied()
                    .unwrap_or("");
                Ok(TmuxOutput {
                    stdout: t.screens.get(target).cloned().unwrap_or_default(),
                })
            }
            _ => Ok(TmuxOutput {
                stdout: String::new(),
            }),
        }
    }
}

/// What the dashboard shows, minus identity and presentation that legitimately differ.
type Shown = Vec<(
    HostHealth,
    Vec<(String, String, String, AgentState, String)>,
)>;

fn shown(s: &UiSnapshot) -> Shown {
    s.hosts
        .iter()
        .map(|h| {
            let mut panes: Vec<_> = h
                .panes
                .iter()
                .map(|p| {
                    (
                        p.session.clone(),
                        p.window.clone(),
                        format!("{:?}", p.agent),
                        p.state,
                        p.why.clone(),
                    )
                })
                .collect();
            panes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            (h.health.clone(), panes)
        })
        .collect()
}

struct Rig {
    rt: tokio::runtime::Runtime,
    script: Script,
    direct: Collector,
    servers: TmuxServers,
    link: Arc<NodeLink>,
    backend: OrchestratedBackend,
    server: flight_transport::ServerHandle,
    stop: tokio::sync::watch::Sender<bool>,
}

fn rig() -> Rig {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let script = Script::default();

    // A: the direct collector.
    let mut registry = HostRegistry::new();
    registry.add_host(HostId::new("local"), Transport::Local);
    let runner: BoxedRunner = Box::new(script.clone());
    registry
        .add_server_with_runner(
            &HostId::new("local"),
            ServerId::new("flight"),
            TmuxEndpoint::named("flight").expect("endpoint"),
            runner,
        )
        .expect("server");
    let direct = Collector::new(registry);

    // B: node -> orchestrator -> client, with real identities and mutual TLS.
    let (node_id, ui_id, orch_id) = (
        Arc::new(Identity::generate().expect("node")),
        Arc::new(Identity::generate().expect("ui")),
        Identity::generate().expect("orchestrator"),
    );
    let orchestrator = orch_id.fingerprint().clone();
    let mut trust = TrustStore::empty();
    trust.authorize(node_id.fingerprint(), "mini-1", Role::Node);
    trust.authorize(ui_id.fingerprint(), "laptop", Role::Ui);
    let server = rt
        .block_on(serve(ServerConfig {
            bind: "127.0.0.1:0".parse().expect("addr"),
            identity: orch_id,
            trust,
            trust_path: None,
            core: OrchestratorConfig::default(),
            incarnation: Incarnation::from_bytes([9; Incarnation::LEN]),
            tick_interval: Duration::from_millis(100),
        }))
        .expect("serve");
    let addr = server.local_addr().to_string();
    let mut servers = TmuxServers::new();
    servers.add(ServerId::new("flight"), Box::new(script.clone()));
    let tmux_control: Arc<TmuxServers> = {
        let mut c = TmuxServers::new();
        c.add(ServerId::new("flight"), Box::new(script.clone()));
        Arc::new(c)
    };
    let session = NodeSession::new(
        NodeCore::new(
            node_id.fingerprint().host_id(),
            Incarnation::from_bytes([1; Incarnation::LEN]),
        ),
        "mini-1",
    );
    let link = Arc::new(NodeLink::new(
        NodeLinkConfig {
            address: addr.clone(),
            identity: node_id,
            orchestrator: orchestrator.clone(),
            servers: vec!["flight".into()],
            heartbeat_interval: Duration::from_millis(500),
            reconnect_min: Duration::from_millis(50),
            reconnect_max: Duration::from_millis(200),
        },
        session,
        tmux_control,
    ));
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    {
        let link = link.clone();
        rt.spawn(async move { link.run(stop_rx).await });
    }
    let backend = OrchestratedBackend::start(ClientConfig {
        address: addr,
        identity: ui_id,
        orchestrator,
    })
    .expect("backend");
    Rig {
        rt,
        script,
        direct,
        servers,
        link,
        backend,
        server,
        stop,
    }
}

impl Rig {
    /// Observe at `t` on both paths and wait for B to catch up with A.
    fn step(&mut self, label: &str, t: u64) -> UiSnapshot {
        let want = self.direct.collect(t);
        self.link.observe(self.servers.observe(t));
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let got = self.backend.snapshot(t);
            if shown(&got) == shown(&want) && self.backend.connected() {
                return got;
            }
            assert!(
                Instant::now() < deadline,
                "{label}: the distributed path never agreed with the direct one\n  direct:      {:#?}\n  distributed: {:#?}",
                shown(&want),
                shown(&got)
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn finish(self) {
        let _ = self.stop.send(true);
        drop(self.backend);
        self.rt.block_on(self.server.shutdown());
    }
}

#[test]
fn the_distributed_path_agrees_with_the_direct_collector_through_a_scripted_session() {
    let mut r = rig();

    r.script.set(
        &[
            ("%1", "claude", 11),
            ("%2", "zsh", 12),
            ("%3", "2.1.138", 13),
        ],
        &[("%1", PERMIT), ("%3", IDLE), ("%2", PERMIT)],
    );
    let first = r.step("agents detected, shell hidden", 1_000);
    assert_eq!(first.hosts.len(), 1);
    assert_eq!(first.hosts[0].panes.len(), 2);

    r.script.set(
        &[
            ("%1", "claude", 11),
            ("%2", "zsh", 12),
            ("%3", "2.1.138", 13),
        ],
        &[("%1", BUSY), ("%3", IDLE)],
    );
    r.step("a state change", 1_010);

    // Finishing while away is Done in both, with the same provenance.
    r.script.set(&[("%1", "claude", 11)], &[("%1", IDLE)]);
    let done = r.step("busy then idle becomes Done", 1_020);
    assert_eq!(done.hosts[0].panes[0].state, AgentState::Done);
    assert_eq!(done.hosts[0].panes[0].why, "finished while away");

    r.step("done persists", 1_030);

    // A pane vanishes, another appears.
    r.script.set(&[("%4", "codex", 14)], &[("%4", PERMIT)]);
    r.step("pane replaced", 1_040);

    // The tmux server goes away: both report it the same way, with no panes.
    r.script.fail("no server running on /tmp/tmux-501/flight");
    let gone = r.step("no tmux server", 1_050);
    assert_eq!(gone.hosts[0].health, HostHealth::NoServer);
    assert!(gone.hosts[0].panes.is_empty());

    r.finish();
}

#[test]
fn previews_match_the_direct_capture() {
    let mut r = rig();
    r.script.set(&[("%1", "claude", 11)], &[("%1", PERMIT)]);
    let snap = r.step("pane up", 2_000);
    let pane = snap.hosts[0].panes[0].pane_ref.clone();
    let distributed = r
        .backend
        .preview(&pane)
        .content
        .expect("distributed preview");
    // The direct collector addresses the same pane under its own host id.
    let mut direct_ref = pane.clone();
    direct_ref.host = HostId::new("local");
    let direct = r
        .direct
        .preview(&direct_ref)
        .content
        .expect("direct preview");
    assert_eq!(distributed, direct);
    assert!(distributed
        .iter()
        .any(|l| l.contains("Do you want to proceed?")));
    r.finish();
}

#[test]
fn the_dashboard_survives_the_orchestrator_going_away_and_shows_last_known_state() {
    let mut r = rig();
    r.script.set(&[("%1", "claude", 11)], &[("%1", PERMIT)]);
    r.step("pane up", 3_000);
    let Rig {
        rt,
        server,
        stop,
        mut backend,
        link,
        ..
    } = r;
    rt.block_on(server.shutdown());
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snap = backend.snapshot(3_001);
        if snap
            .hosts
            .first()
            .is_some_and(|h| matches!(h.health, HostHealth::Unreachable(_)))
        {
            // The orchestrator row is explicit; the node and its pane remain, marked stale.
            let node = snap
                .hosts
                .iter()
                .find(|h| h.label == "mini-1")
                .expect("node still listed");
            assert_eq!(node.health, HostHealth::Stale);
            assert_eq!(node.panes.len(), 1, "last-known pane stays visible");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "never noticed the orchestrator was gone"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = stop.send(true);
    drop(link);
}
