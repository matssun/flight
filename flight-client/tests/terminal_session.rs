// SPDX-License-Identifier: MIT

//! Enter on a pane of another machine, end to end over real mutual TLS and a real tmux server
//! on a private socket (skipped without tmux): reveal, open a terminal, show it through the
//! relay, leave. There is no ssh and no path from the UI to the node anywhere in this file.

use flight_classify::AgentKind;
use flight_client::{
    Attachment, Binding, ClientConfig, Handoff, LinkHost, LocalTerminal, OpenFailure, OpenRequest,
    OrchestratedBackend, RemoteOps, SessionConfig, SessionOutcome, SessionStart, SurfaceHost,
    SurfaceSession, Switcher, TerminalEnd,
};
use flight_node::{NodeCore, NodeSession, PaneObservation, Round, ServerOutcome, TmuxServers};
use flight_orchestrator::OrchestratorConfig;
use flight_proto::{ui_request_body, ExitReasonCode, Incarnation, TerminalLease, UiRequest};
use flight_state::{AgentState, HostId, PaneId, PaneRef, ServerId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use flight_transport::{serve, NodeLink, NodeLinkConfig, ServerConfig, ServerHandle, UiClient};
use flight_trust::{Identity, Role, TrustStore};
use flight_ui::{Backend, PaneView, SurfaceChoice, WorkspaceKey};
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
    link: Arc<NodeLink>,
    observed: Vec<PaneObservation>,
    /// (pane id, pid) of the two panes of session `work`.
    panes: Vec<(String, u32)>,
}

fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

fn start(tag: &str, command: &str, terminals: bool) -> Rig {
    start_with(tag, command, terminals, false)
}

/// `windows`: the second pane is a window of its own (a workspace's agent and shell), not a
/// split of the first.
fn start_with(tag: &str, command: &str, terminals: bool, windows: bool) -> Rig {
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
    let second = if windows {
        ["new-window", "-d", "-t", "work:"]
    } else {
        ["split-window", "-d", "-t", "work:"]
    };
    tmux.runner()
        .run(&[second[0], second[1], second[2], second[3], command])
        .expect("second pane");
    let listing = tmux
        .runner()
        .run(&[
            "list-panes",
            "-s",
            "-t",
            "work:",
            "-F",
            "#{pane_id} #{pane_pid}",
        ])
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
    // One workspace with an agent surface (the first pane) and a shell surface (the second).
    let observed: Vec<PaneObservation> = panes
        .iter()
        .enumerate()
        .map(|(n, (id, pid))| PaneObservation {
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
            placement: flight_state::RawPlacement {
                workspace_id: "w-test".into(),
                surface_id: format!("s-{n}"),
                surface_kind: if n == 0 { "agent" } else { "shell" }.into(),
                window_id: "@0".into(),
                session_id: "$0".into(),
                session_path: "/tmp".into(),
            },
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
        outcome: ServerOutcome::Observed(observed.clone()),
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
        link,
        observed,
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

    /// Enter on pane `i`: the terminal the dashboard hands over, and the process it is for.
    fn enter(&mut self, i: usize) -> (Vec<u8>, Binding) {
        let view = self.view(i, 0);
        self.backend.switch_to(&view).expect("switch");
        match self.backend.handoff().take() {
            Some(Handoff::Terminal { id, binding, .. }) => (id, binding),
            other => panic!("expected a terminal handoff, got {other:?}"),
        }
    }

    fn workspace(&self) -> WorkspaceKey {
        WorkspaceKey {
            host: self.host.clone(),
            workspace: flight_state::WorkspaceId::new("w-test"),
        }
    }

    /// Show terminal `id` in a session over the real link, with plain channels.
    fn show(&self, (id, binding): &(Vec<u8>, Binding)) -> Shown {
        let host = Arc::new(LinkHost::new(self.backend.clone(), self.workspace()));
        self.show_with(host, id, binding, |_| {})
    }

    fn show_with<H: SurfaceHost>(
        &self,
        host: Arc<H>,
        id: &[u8],
        binding: &Binding,
        tweak: impl FnOnce(&mut SessionConfig),
    ) -> Shown {
        let (input_tx, input_rx) = mpsc::channel(8);
        let (resize_tx, resize_rx) = mpsc::channel(4);
        let (output_tx, output_rx) = mpsc::channel(2);
        let mut config = SessionConfig::new(Arc::new(|_| {}));
        tweak(&mut config);
        let session = SurfaceSession::new(host, config);
        let start = SessionStart {
            id: id.to_vec(),
            choice: SurfaceChoice::Agent,
            binding: binding.clone(),
            typed_ahead: Vec::new(),
            size: (100, 30),
        };
        let task = self.rt.spawn(session.run(
            LocalTerminal {
                input: input_rx,
                resizes: resize_rx,
                output: output_tx,
            },
            start,
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
    task: tokio::task::JoinHandle<SessionOutcome>,
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
        .expect("the session ends")
        .expect("task")
        .end
}

#[test]
fn enter_reveals_the_pane_opens_a_terminal_and_the_escape_leaves_cleanly() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("enter", "cat", true);
    // Pane 0 is shown first; Enter on pane 1 must select it before the terminal attaches.
    assert_ne!(rig.active_pane(), rig.panes[1].0);
    let entered = rig.enter(1);
    assert_eq!(rig.active_pane(), rig.panes[1].0, "the pane was revealed");

    let mut shown = rig.show(&entered);
    rig.wait(
        "tmux client attached",
        |r| matches!(r.clients().as_slice(), [only] if only.starts_with("flight-view-")),
    );
    rig.rt
        .block_on(shown.input.send(b"typed-through-the-session\r".to_vec()))
        .expect("input");
    read_until(&rig, &mut shown, "typed-through-the-session");
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
    let entered = rig.enter(0);
    let shown = rig.show(&entered);
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
fn a_remote_detach_ends_the_session_with_the_reason() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("detach", "cat", true);
    let entered = rig.enter(0);
    let mut shown = rig.show(&entered);
    // The user's terminal is reading, so the relay can get to the end of the stream.
    let mut output = std::mem::replace(&mut shown.output, mpsc::channel(1).1);
    rig.rt
        .spawn(async move { while output.recv().await.is_some() {} });
    // A client is listed before it is attached to its session; detaching then does nothing.
    rig.wait(
        "attached to its session",
        |r| matches!(r.clients().as_slice(), [only] if only.starts_with("flight-view-")),
    );
    rig.tmux
        .runner()
        .run(&["detach-client", "-s", &rig.clients()[0]])
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
fn losing_the_orchestrator_mid_session_ends_the_session_and_hangs_the_node_up() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("orchgone", "cat", true);
    let entered = rig.enter(0);
    let shown = rig.show(&entered);
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
    // A presenter whose lease rides a control connection the test can kill. Its lease is
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
    let entered = rig.enter(0);
    let renewals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let host = Arc::new(KillableLease {
        inner: LinkHost::new(rig.backend.clone(), rig.workspace()),
        control: control.clone(),
        renewals: renewals.clone(),
    });
    let mut shown = rig.show_with(host, &entered.0, &entered.1, |c| {
        c.lease_period = Duration::from_secs(1);
    });
    rig.wait(
        "tmux client attached",
        |r| matches!(r.clients().as_slice(), [only] if only.starts_with("flight-view-")),
    );
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
        let entered = rig.enter(0);
        let shown = rig.show(&entered);
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

/// A host that attaches over the real link but renews its lease on a connection of its own, so
/// that the test can make the renewal fail the way a dead control connection does.
struct KillableLease {
    inner: LinkHost,
    control: Arc<std::sync::Mutex<Option<UiClient>>>,
    renewals: Arc<std::sync::atomic::AtomicUsize>,
}

impl SurfaceHost for KillableLease {
    async fn open(&self, request: OpenRequest) -> Result<Attachment, OpenFailure> {
        self.inner.open(request).await
    }

    async fn connect(
        &self,
        id: Vec<u8>,
        choice: SurfaceChoice,
        binding: Binding,
    ) -> Result<Attachment, OpenFailure> {
        self.inner.connect(id, choice, binding).await
    }

    fn renew(&self, attachment: &[u8]) -> Result<(), String> {
        let guard = self.control.lock().unwrap_or_else(|p| p.into_inner());
        let client = guard.as_ref().ok_or("it was dropped")?;
        client
            .send(UiRequest {
                body: Some(ui_request_body::Body::TerminalLease(TerminalLease {
                    terminal_id: attachment.to_vec(),
                })),
            })
            .map_err(|e| e.to_string())?;
        self.renewals
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

/// Text on the screen of pane `i`.
fn pane_text(rig: &Rig, i: usize) -> String {
    rig.tmux
        .runner()
        .run(&["capture-pane", "-p", "-t", rig.panes[i].0.as_str()])
        .map(|o| o.stdout)
        .unwrap_or_default()
}

#[test]
fn switching_surface_stays_in_the_session_keeps_both_surfaces_running_and_orders_input() {
    if !tmux_available() {
        return;
    }
    // Surfaces are windows (a workspace's agent and shell), as in the product: panes of one
    // window share their active pane, so two views could not show two of them independently.
    let mut rig = start_with("switch", "cat", true, true);
    let entered = rig.enter(0);
    let shown = rig.show(&entered);
    rig.wait("tmux client attached", |r| r.clients().len() == 1);

    // Typed for the agent, then the switch, then typed for the shell, all in one burst: the
    // shell is not attached yet when the second part is typed, and it must still get it.
    rig.rt
        .block_on(
            shown
                .input
                .send(b"for-the-agent\r\x00sfor-the-shell\r".to_vec()),
        )
        .expect("input");
    rig.wait("the shell has what was typed for it", |r| {
        pane_text(r, 1).contains("for-the-shell")
    });
    // (The agent's own attachment is a different stream: its text may land a moment after the
    // shell's. Each surface is ordered; the two are not ordered against each other.)
    rig.wait("the agent has what was typed for it", |r| {
        pane_text(r, 0).contains("for-the-agent")
    });
    assert!(
        !pane_text(&rig, 0).contains("for-the-shell"),
        "misdelivered"
    );
    assert!(
        !pane_text(&rig, 1).contains("for-the-agent"),
        "misdelivered"
    );
    // One client, on the shell; the agent's attachment was let go once the shell was up.
    rig.wait("one client left", |r| r.clients().len() == 1);

    // And back: both surfaces kept their own state.
    rig.rt
        .block_on(shown.input.send(b"\x00aagain-agent\r".to_vec()))
        .expect("input");
    rig.wait("the agent has what was typed for it", |r| {
        pane_text(r, 0).contains("again-agent")
    });
    assert!(pane_text(&rig, 0).contains("for-the-agent"));
    assert!(pane_text(&rig, 1).contains("for-the-shell"));
    assert!(!pane_text(&rig, 1).contains("again-agent"), "misdelivered");

    rig.rt
        .block_on(shown.input.send(b"\x00q".to_vec()))
        .expect("input");
    let outcome = rig
        .rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(10), shown.task).await })
        .expect("ends")
        .expect("task");
    assert_eq!(outcome.end, TerminalEnd::UserLeft);
    assert_eq!(outcome.shown, Some(SurfaceChoice::Agent));
    assert_eq!(outcome.undelivered, 0);
    rig.wait("no tmux client left", |r| r.clients().is_empty());
    rig.wait("table empty", |r| r.terminals_open() == 0);
}

#[test]
fn a_switch_to_a_surface_that_is_gone_keeps_the_user_where_they_are_and_delivers_nothing_wrong() {
    if !tmux_available() {
        return;
    }
    let mut rig = start("gone", "cat", true);
    let entered = rig.enter(0);
    let shown = rig.show(&entered);
    rig.wait("tmux client attached", |r| r.clients().len() == 1);
    // The shell goes away, and the node tells the orchestrator.
    rig.tmux
        .runner()
        .run(&["kill-pane", "-t", rig.panes[1].0.as_str()])
        .expect("kill");
    rig.link.observe(vec![Round {
        server: ServerId::new("flight"),
        now: 2,
        outcome: ServerOutcome::Observed(rig.observed.iter().take(1).cloned().collect()),
    }]);
    let mut link = rig.backend.clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    while link
        .snapshot(0)
        .hosts
        .iter()
        .map(|h| h.panes.len())
        .sum::<usize>()
        != 1
    {
        assert!(
            Instant::now() < deadline,
            "the link never dropped the shell"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    rig.rt
        .block_on(shown.input.send(b"\x00snot-for-the-agent\r".to_vec()))
        .expect("input");
    std::thread::sleep(Duration::from_millis(500));
    rig.rt
        .block_on(shown.input.send(b"for-the-agent\r".to_vec()))
        .expect("input");
    rig.wait("the agent has what was typed for it", |r| {
        pane_text(r, 0).contains("for-the-agent")
    });
    assert!(
        !pane_text(&rig, 0).contains("not-for-the-agent"),
        "input typed for a surface that could not be reached went to another one"
    );
    rig.rt
        .block_on(shown.input.send(b"\x00q".to_vec()))
        .expect("input");
    let outcome = rig
        .rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(10), shown.task).await })
        .expect("ends")
        .expect("task");
    assert_eq!(outcome.end, TerminalEnd::UserLeft);
    assert_eq!(outcome.undelivered, "not-for-the-agent\r".len());
}

#[test]
fn switching_between_windows_never_moves_the_workspace_s_own_session_or_the_other_view() {
    if !tmux_available() {
        return;
    }
    let mut rig = start_with("windows", "cat", true, true);
    let own = |r: &Rig| {
        r.tmux
            .runner()
            .run(&["display-message", "-p", "-t", "=work:", "#{window_index}"])
            .map(|o| o.stdout.trim().to_owned())
            .unwrap_or_default()
    };
    let before = own(&rig);
    let entered = rig.enter(0);
    let shown = rig.show(&entered);
    rig.wait("tmux client attached", |r| r.clients().len() == 1);
    rig.rt
        .block_on(
            shown
                .input
                .send(b"in-window-0\r\x00sin-window-1\r\x00ain-window-0-again\r".to_vec()),
        )
        .expect("input");
    rig.wait("every part arrived where it was typed", |r| {
        pane_text(r, 0).contains("in-window-0-again") && pane_text(r, 1).contains("in-window-1")
    });
    let (agent, shell) = (pane_text(&rig, 0), pane_text(&rig, 1));
    assert!(
        agent.contains("in-window-0") && !agent.contains("in-window-1"),
        "{agent}"
    );
    assert!(
        shell.contains("in-window-1") && !shell.contains("in-window-0"),
        "{shell}"
    );
    assert_eq!(own(&rig), before, "the workspace's own session never moved");
    rig.rt
        .block_on(shown.input.send(b"\x00q".to_vec()))
        .expect("input");
    assert_eq!(finish(&rig, shown), TerminalEnd::UserLeft);
    rig.wait("no tmux client left", |r| r.clients().is_empty());
    let sessions = rig
        .tmux
        .runner()
        .run(&["list-sessions", "-F", "#{session_name}"])
        .unwrap()
        .stdout;
    assert_eq!(
        sessions.trim(),
        "work",
        "no view is left behind: {sessions}"
    );
}
