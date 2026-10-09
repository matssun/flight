// SPDX-License-Identifier: MIT

//! Enter on a pane of another machine, end to end over real mutual TLS and a real tmux server
//! on a private socket (skipped without tmux): reveal, open a terminal, show it through the
//! relay, leave. There is no ssh and no path from the UI to the node anywhere in this file.

use flight_classify::AgentKind;
use flight_client::{
    relay, ClientConfig, Handoff, Lease, OrchestratedBackend, RemoteOps, Switcher, TerminalEnd,
};
use flight_node::{NodeCore, NodeSession, PaneObservation, Round, ServerOutcome, TmuxServers};
use flight_orchestrator::OrchestratorConfig;
use flight_proto::{ui_request_body, ExitReasonCode, Incarnation, TerminalLease, UiRequest};
use flight_state::{AgentState, HostId, PaneId, PaneRef, ServerId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use flight_transport::{
    serve, NodeLink, NodeLinkConfig, ServerConfig, ServerHandle, TerminalClient, UiClient,
};
use flight_trust::{Identity, Role, TrustStore};
use flight_ui::{Backend, PaneView};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

/// These tests start tmux servers and PTYs in one process. A child forked by one test can
/// briefly inherit a descriptor another test's PTY depends on, which delays the end-of-file
/// that tells a node its tmux client is gone, so they run one at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Rig {
    _serial: std::sync::MutexGuard<'static, ()>,
    rt: tokio::runtime::Runtime,
    server: Option<ServerHandle>,
    config: ClientConfig,
    backend: OrchestratedBackend,
    tmux: Tmux,
    host: HostId,
    stop: watch::Sender<bool>,
    /// (pane id, pid) of the two panes of session `work`.
    panes: Vec<(String, u32)>,
}

fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

fn start(tag: &str, command: &str, terminals: bool) -> Rig {
    let serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let name = format!("flight-test-{}-{tag}", std::process::id());
    let endpoint = TmuxEndpoint::named(&name).expect("endpoint");
    let tmux = Tmux::new(endpoint.clone());
    tmux.runner()
        .run(&[
            "new-session",
            "-d",
            "-s",
            "work",
            "-x",
            "100",
            "-y",
            "30",
            command,
        ])
        .expect("session");
    tmux.runner()
        .run(&["split-window", "-d", "-t", "work:", command])
        .expect("split");
    let listing = tmux
        .runner()
        .run(&["list-panes", "-t", "work:", "-F", "#{pane_id} #{pane_pid}"])
        .expect("panes")
        .stdout;
    let panes: Vec<(String, u32)> = listing
        .lines()
        .filter_map(|l| {
            let (id, pid) = l.split_once(' ')?;
            Some((id.to_owned(), pid.parse().ok()?))
        })
        .collect();

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
    servers.add(
        ServerId::new("flight"),
        Box::new(SystemRunner::new(endpoint.clone())),
    );
    servers.allow_terminal(ServerId::new("flight"), endpoint);
    let host = node_id.fingerprint().host_id();
    let mut session = NodeSession::new(
        NodeCore::new(host.clone(), Incarnation::from_bytes([1; Incarnation::LEN])),
        "mini-1",
    );
    if !terminals {
        session = session.without_terminal();
    }
    let link = Arc::new(NodeLink::new(
        NodeLinkConfig {
            address: addr.clone(),
            identity: node_id.clone(),
            orchestrator: orchestrator.clone(),
            servers: vec!["flight".to_owned()],
            heartbeat_interval: Duration::from_millis(500),
            reconnect_min: Duration::from_millis(50),
            reconnect_max: Duration::from_millis(200),
        },
        session,
        Arc::new(servers),
    ));
    let (stop, stop_rx) = watch::channel(false);
    {
        let link = link.clone();
        rt.spawn(async move { link.run(stop_rx).await });
    }
    let observed: Vec<PaneObservation> = panes
        .iter()
        .map(|(id, pid)| PaneObservation {
            pane: PaneId::new(id.as_str()),
            pid: *pid,
            agent: AgentKind::Claude,
            session: "work".into(),
            window: "agent".into(),
            path: "/tmp".into(),
            command: "claude".into(),
            title: String::new(),
            focused: false,
            screen_lines: vec!["Done!".into(), String::new(), "❯".into()],
            placement: Default::default(),
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while server
        .fleet_snapshot()
        .nodes
        .iter()
        .all(|n| n.status != flight_proto::NodeStatusCode::Online as i32)
    {
        assert!(Instant::now() < deadline, "node never came online");
        std::thread::sleep(Duration::from_millis(50));
    }
    link.observe(vec![Round {
        server: ServerId::new("flight"),
        now: 1,
        outcome: ServerOutcome::Observed(observed),
    }]);

    let config = ClientConfig {
        address: addr,
        identity: ui_id,
        orchestrator,
    };
    let mut backend = OrchestratedBackend::start(config.clone()).expect("backend");
    // No node on this machine: every pane is remote.
    backend.set_switching(Switcher::new(None));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !backend.connected()
        || backend
            .snapshot(0)
            .hosts
            .iter()
            .map(|h| h.panes.len())
            .sum::<usize>()
            < panes.len()
    {
        assert!(
            Instant::now() < deadline,
            "the dashboard never saw the panes"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Rig {
        _serial: serial,
        rt,
        server: Some(server),
        config,
        backend,
        tmux,
        host,
        stop,
        panes,
    }
}

impl Rig {
    fn relays_open(&self) -> usize {
        self.server.as_ref().map_or(0, |s| s.terminal_relays())
    }

    fn terminals_open(&self) -> usize {
        self.server.as_ref().map_or(0, |s| s.terminals_open())
    }

    fn view(&self, i: usize, pid_offset: u32) -> PaneView {
        let (id, pid) = &self.panes[i];
        PaneView {
            pane_ref: PaneRef {
                host: self.host.clone(),
                server: ServerId::new("flight"),
                pane: PaneId::new(id.as_str()),
            },
            session: "work".into(),
            window: "agent".into(),
            agent: AgentKind::Claude,
            state: AgentState::Done,
            why: String::new(),
            title: String::new(),
            pid: pid + pid_offset,
            workspace: flight_state::WorkspaceId::new("w-test"),
            surface: flight_state::SurfaceId::new(format!("s-{id}")),
            kind: flight_ui::SurfaceKind::Agent(AgentKind::Claude),
            root: "/work".into(),
        }
    }

    fn clients(&self) -> Vec<String> {
        self.tmux
            .runner()
            .run(&["list-clients", "-F", "#{client_session}"])
            .map(|o| o.stdout.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn active_pane(&self) -> String {
        self.tmux
            .runner()
            .run(&["display-message", "-p", "-t", "work:", "#{pane_id}"])
            .expect("active")
            .stdout
            .trim()
            .to_owned()
    }

    fn wait(&self, what: &str, check: impl Fn(&Rig) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check(self) {
            assert!(Instant::now() < deadline, "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Enter on pane `i`: the id of the terminal the dashboard hands over.
    fn enter(&mut self, i: usize) -> Vec<u8> {
        let view = self.view(i, 0);
        self.backend.switch_to(&view).expect("switch");
        match self.backend.handoff().take() {
            Some(Handoff::Terminal { id, .. }) => id,
            other => panic!("expected a terminal handoff, got {other:?}"),
        }
    }

    /// Attach the relay to terminal `id` with plain channels.
    fn show(&self, id: &[u8]) -> Shown {
        let lease = self
            .rt
            .block_on(Lease::connect(&self.config, id))
            .expect("lease");
        self.show_with(id, lease)
    }

    fn show_with(&self, id: &[u8], lease: Lease) -> Shown {
        let client = self
            .rt
            .block_on(TerminalClient::connect_ui(
                &self.config.address,
                &self.config.identity,
                &self.config.orchestrator,
                id,
            ))
            .expect("terminal");
        let (sender, receiver) = client.split();
        let (input_tx, input_rx) = mpsc::channel(8);
        let (resize_tx, resize_rx) = mpsc::channel(4);
        let (output_tx, output_rx) = mpsc::channel(2);
        let task = self.rt.spawn(relay(
            sender,
            receiver,
            input_rx,
            resize_rx,
            output_tx,
            || {},
            lease,
            None,
        ));
        Shown {
            input: input_tx,
            resize: resize_tx,
            output: output_rx,
            task,
        }
    }
}

struct Shown {
    input: mpsc::Sender<Vec<u8>>,
    resize: mpsc::Sender<(u16, u16)>,
    output: mpsc::Receiver<Vec<u8>>,
    task: tokio::task::JoinHandle<TerminalEnd>,
}

impl Rig {
    /// Stop the orchestrator (it is gone for good in this test).
    fn stop_orchestrator(&mut self) {
        // `shutdown` consumes the handle; swap in nothing by moving it out of an Option.
        if let Some(server) = self.server.take() {
            self.rt.block_on(server.shutdown());
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        let _ = self.tmux.kill_server();
    }
}

fn read_until(rig: &Rig, shown: &mut Shown, needle: &str) {
    rig.rt.block_on(async {
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(chunk)) =
                tokio::time::timeout(Duration::from_millis(100), shown.output.recv()).await
            {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains(needle) {
                    return;
                }
            }
        }
        panic!("never saw {needle:?} in {seen:?}");
    });
}

fn finish(rig: &Rig, shown: Shown) -> TerminalEnd {
    rig.rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(10), shown.task).await })
        .expect("the relay ends")
        .expect("task")
}

#[test]
fn enter_reveals_the_pane_opens_a_terminal_and_the_escape_leaves_cleanly() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("enter", "cat", true);
    // Pane 0 is shown first; Enter on pane 1 must select it before the terminal attaches.
    assert_ne!(rig.active_pane(), rig.panes[1].0);
    let id = rig.enter(1);
    assert_eq!(rig.active_pane(), rig.panes[1].0, "the pane was revealed");

    let mut shown = rig.show(&id);
    rig.wait("tmux client attached", |r| {
        r.clients() == vec!["work".to_owned()]
    });
    rig.rt
        .block_on(shown.input.send(b"typed-through-the-relay\r".to_vec()))
        .expect("input");
    read_until(&rig, &mut shown, "typed-through-the-relay");
    rig.rt
        .block_on(shown.resize.send((70, 20)))
        .expect("resize");
    rig.wait("resized", |r| {
        r.tmux
            .runner()
            .run(&["list-clients", "-F", "#{client_width}x#{client_height}"])
            .is_ok_and(|o| o.stdout.trim() == "70x20")
    });

    // A literal Ctrl-Space reaches the remote; Ctrl-Space q leaves.
    rig.rt
        .block_on(shown.input.send(b"\x00\x00".to_vec()))
        .expect("input");
    rig.rt
        .block_on(shown.input.send(b"\x00q".to_vec()))
        .expect("input");
    assert_eq!(finish(&rig, shown), TerminalEnd::UserLeft);
    rig.wait("no tmux client left", |r| r.clients().is_empty());
    rig.wait("table empty", |r| r.terminals_open() == 0);
    rig.tmux
        .runner()
        .run(&["has-session", "-t", "=work"])
        .expect("the session is untouched");
}

#[test]
fn the_escape_works_while_the_users_terminal_accepts_nothing_and_memory_stays_bounded() {
    if !tmux_available() {
        return;
    }
    let mut rig = start(
        "wedge",
        "sh -c 'while :; do echo flood-flood-flood-flood-flood-flood; done'",
        true,
    );
    let id = rig.enter(0);
    let shown = rig.show(&id);
    rig.wait("tmux client attached", |r| r.clients().len() == 1);
    // Nobody reads `shown.output`: the user's terminal is stuck. Let it run.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        shown.output.len() <= 2,
        "the relay queued {} chunks",
        shown.output.len()
    );
    let tmux_pid: u32 = rig
        .tmux
        .runner()
        .run(&["display-message", "-p", "#{pid}"])
        .expect("pid")
        .stdout
        .trim()
        .parse()
        .expect("pid");
    let rss = |pid: u32| -> u64 {
        Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
            .unwrap_or(0)
    };
    let (tmux_before, mine_before) = (rss(tmux_pid), rss(std::process::id()));
    std::thread::sleep(Duration::from_secs(3));
    assert!(rss(tmux_pid) < tmux_before + 40_000, "tmux grew");
    assert!(
        rss(std::process::id()) < mine_before + 40_000,
        "the client side grew"
    );

    // The local escape still works.
    rig.rt
        .block_on(shown.input.send(b"\x00q".to_vec()))
        .expect("input");
    assert_eq!(finish(&rig, shown), TerminalEnd::UserLeft);
    rig.wait("no tmux client left", |r| r.clients().is_empty());
    rig.wait("table empty", |r| r.terminals_open() == 0);
}

#[test]
fn a_remote_detach_ends_the_relay_with_the_reason() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("detach", "cat", true);
    let id = rig.enter(0);
    let mut shown = rig.show(&id);
    // The user's terminal is reading, so the relay can get to the end of the stream.
    let mut output = std::mem::replace(&mut shown.output, mpsc::channel(1).1);
    rig.rt
        .spawn(async move { while output.recv().await.is_some() {} });
    // A client is listed before it is attached to its session; detaching then does nothing.
    rig.wait("attached to its session", |r| {
        r.clients() == vec!["work".to_owned()]
    });
    rig.tmux
        .runner()
        .run(&["detach-client", "-s", "work"])
        .expect("detach");
    match finish(&rig, shown) {
        TerminalEnd::Exited { reason, .. } => assert_eq!(reason, ExitReasonCode::ClientExited),
        other => panic!("{other:?}"),
    }
    rig.wait("table empty", |r| r.terminals_open() == 0);
}

#[test]
fn a_pane_that_changed_since_it_was_listed_reveals_nothing_and_opens_nothing() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("stale", "cat", true);
    let before = rig.active_pane();
    let view = rig.view(1, 1);
    let err = rig.backend.switch_to(&view).expect_err("stale");
    assert!(err.contains("changed"), "{err}");
    assert!(err.starts_with("could not select the pane"), "{err}");
    assert_eq!(rig.active_pane(), before, "nothing was revealed");
    assert!(rig.backend.handoff().take().is_none());
    assert_eq!(rig.terminals_open(), 0);
    assert!(rig.clients().is_empty());
}

#[test]
fn a_node_that_does_not_offer_terminals_still_reveals_and_says_which_stage_failed() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("noterm", "cat", false);
    let view = rig.view(1, 0);
    let err = rig.backend.switch_to(&view).expect_err("no terminals");
    assert!(err.starts_with("pane selected, but cannot attach"), "{err}");
    assert!(err.contains("cannot open a terminal"), "{err}");
    assert_eq!(rig.active_pane(), rig.panes[1].0, "the reveal did happen");
    assert!(rig.backend.handoff().take().is_none());
    assert_eq!(rig.terminals_open(), 0);
}

#[test]
fn nothing_in_the_remote_path_needs_ssh() {
    // The type that carries a remote switch is a pair of orchestrator requests, nothing else.
    fn assert_remote_ops<T: RemoteOps>() {}
    assert_remote_ops::<NoSsh>();
    struct NoSsh;
    impl RemoteOps for NoSsh {
        fn reveal(&mut self, _: &PaneRef, _: u32) -> Result<(), String> {
            Ok(())
        }
        fn open_terminal(&mut self, _: &PaneRef, _: u32) -> Result<Vec<u8>, String> {
            Ok(vec![0; 16])
        }
    }
}

#[test]
fn a_pane_id_reused_by_a_restarted_tmux_is_refused_even_though_the_node_has_not_noticed() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("reuse", "cat", true);
    let stale = rig.view(1, 0);
    // tmux restarts and hands the same pane ids to new processes; the node's last observation
    // (and so the dashboard's pid) is now a lie.
    let _ = rig.tmux.runner().run(&["kill-server"]);
    // The old server may still be shutting down; starting a new one then fails.
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig
        .tmux
        .runner()
        .run(&[
            "new-session",
            "-d",
            "-s",
            "work",
            "-x",
            "100",
            "-y",
            "30",
            "cat",
        ])
        .is_err()
    {
        assert!(Instant::now() < deadline, "tmux would not restart");
        std::thread::sleep(Duration::from_millis(50));
    }
    rig.tmux
        .runner()
        .run(&["split-window", "-d", "-t", "work:", "cat"])
        .expect("split");
    let err = rig.backend.switch_to(&stale).expect_err("stale");
    assert!(err.contains("changed"), "{err}");
    assert!(rig.backend.handoff().take().is_none());
    assert_eq!(rig.terminals_open(), 0);
    assert!(rig.clients().is_empty());
}

#[test]
fn losing_the_orchestrator_mid_session_ends_the_relay_and_hangs_the_node_up() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("orchgone", "cat", true);
    let id = rig.enter(0);
    let shown = rig.show(&id);
    rig.wait("tmux client attached", |r| r.clients().len() == 1);
    rig.stop_orchestrator();
    match finish(&rig, shown) {
        TerminalEnd::Lost(_) | TerminalEnd::Exited { .. } => {}
        other => panic!("{other:?}"),
    }
    rig.wait("the node hung its tmux client up", |r| {
        r.clients().is_empty()
    });
    rig.tmux
        .runner()
        .run(&["has-session", "-t", "=work"])
        .expect("the session is untouched");
}

#[test]
fn a_presenter_whose_control_connection_dies_stops_renewing_and_the_terminal_goes() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("leasectl", "cat", true);
    // The presenter's dedicated control connection, which the test can kill. Its lease is
    // renewed every second; the orchestrator's lifetime (15 s) and stall limit (30 s) are
    // far away, so only the presenter noticing can end this quickly.
    let control = Arc::new(std::sync::Mutex::new(Some(
        rig.rt
            .block_on(UiClient::connect(
                &rig.config.address,
                &rig.config.identity,
                &rig.config.orchestrator,
            ))
            .expect("control"),
    )));
    let id = rig.enter(0);
    let renewals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let lease = {
        let (control, renewals, id) = (control.clone(), renewals.clone(), id.clone());
        Lease {
            period: Duration::from_secs(1),
            renew: Box::new(move || {
                let guard = control.lock().unwrap_or_else(|p| p.into_inner());
                let client = guard.as_ref().ok_or("it was dropped")?;
                client
                    .send(UiRequest {
                        body: Some(ui_request_body::Body::TerminalLease(TerminalLease {
                            terminal_id: id.clone(),
                        })),
                    })
                    .map_err(|e| e.to_string())?;
                renewals.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            }),
        }
    };
    let mut shown = rig.show_with(&id, lease);
    rig.wait("tmux client attached", |r| {
        r.clients() == vec!["work".to_owned()]
    });
    // Normal traffic, and the lease is being renewed.
    rig.rt
        .block_on(shown.input.send(b"healthy-traffic\r".to_vec()))
        .expect("input");
    read_until(&rig, &mut shown, "healthy-traffic");
    rig.wait("renewals", |_| {
        renewals.load(std::sync::atomic::Ordering::Relaxed) >= 2
    });

    // The control connection dies; the terminal stream itself is untouched.
    let killed = Instant::now();
    drop(control.lock().unwrap_or_else(|p| p.into_inner()).take());
    let end = finish(&rig, shown);
    let took = killed.elapsed();
    eprintln!("presenter ended after {took:?}: {end}");
    assert!(
        matches!(&end, TerminalEnd::Lost(why) if why.contains("control connection")),
        "{end:?}"
    );
    let stopped_at = renewals.load(std::sync::atomic::Ordering::Relaxed);

    rig.wait("no tmux client left", |r| r.clients().is_empty());
    rig.wait("table empty", |r| r.terminals_open() == 0);
    rig.wait("relay released", |r| r.relays_open() == 0);
    assert!(took < Duration::from_secs(5), "took {took:?}");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        renewals.load(std::sync::atomic::Ordering::Relaxed),
        stopped_at,
        "the presenter kept renewing"
    );

    // No node slot was consumed: the node's limit is four, and five more terminals open.
    for n in 0..5 {
        let id = rig.enter(0);
        let shown = rig.show(&id);
        rig.wait("tmux client attached", |r| r.clients().len() == 1);
        rig.rt
            .block_on(shown.input.send(b"\x00q".to_vec()))
            .expect("input");
        assert_eq!(finish(&rig, shown), TerminalEnd::UserLeft, "terminal {n}");
        rig.wait("no tmux client left", |r| r.clients().is_empty());
        rig.wait("table empty", |r| r.terminals_open() == 0);
    }
    rig.wait("relays released", |r| r.relays_open() == 0);
}
