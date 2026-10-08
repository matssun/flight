// SPDX-License-Identifier: MIT

//! Terminals end to end over real mutual TLS and a real tmux server on a private socket
//! (skipped without tmux): UI -> orchestrator -> node -> PTY -> tmux client -> pane, with the
//! orchestrator copying bytes it never interprets.

mod support;

use flight_node::TmuxServers;
use flight_proto::{
    command_kind as ck, response_result, terminal_body, ui_event_body, ui_request_body, Command,
    ErrorKindCode, ExitReasonCode, NodeStatusCode, PaneRefMsg, Request, Response, Subscribe,
    TerminalClose, TerminalFrame, TerminalLease, TerminalResize, UiRequest,
};
use flight_state::ServerId;
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use flight_transport::{ServerHandle, TerminalClient, UiClient};
use flight_trust::{Fingerprint, Identity};
use std::process::Command as Process;
use std::sync::Arc;
use std::time::Duration;
use support::*;
use tokio::sync::watch;

/// These tests start tmux servers and PTYs in one process. A child forked by one test can
/// briefly inherit a descriptor another test's PTY depends on, which delays the end-of-file
/// that tells a node its tmux client is gone, so they run one at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Rig {
    _serial: tokio::sync::MutexGuard<'static, ()>,
    server: ServerHandle,
    addr: String,
    orch: Fingerprint,
    ui_id: Identity,
    node_fp: Fingerprint,
    tmux: Tmux,
    tmux_name: String,
    stop: watch::Sender<bool>,
    /// The node runs on its own runtime so a test can make it vanish without a goodbye.
    node_runtime: Option<tokio::runtime::Runtime>,
    ui: UiClient,
    next_request: u64,
    lease_ttl: u64,
    leases: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

fn tmux_available() -> bool {
    Process::new("tmux").arg("-V").output().is_ok()
}

impl Rig {
    /// A node whose tmux has a session `work` running `command`; its pane is published with
    /// the real process id.
    async fn start(tag: &str, command: &str) -> (Self, String, u32) {
        Self::start_with(tag, command, Duration::from_secs(30)).await
    }

    async fn start_with(tag: &str, command: &str, stall: Duration) -> (Self, String, u32) {
        Self::start_full(tag, command, stall, 15).await
    }

    /// `ttl`: how many seconds a terminal lives after its UI's last lease.
    async fn start_full(
        tag: &str,
        command: &str,
        stall: Duration,
        ttl: u64,
    ) -> (Self, String, u32) {
        let serial = ONE_AT_A_TIME.lock().await;
        let tmux_name = format!("flight-test-{}-{tag}", std::process::id());
        let endpoint = TmuxEndpoint::named(&tmux_name).unwrap();
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
            .unwrap();
        let listing = tmux
            .runner()
            .run(&["list-panes", "-t", "work:", "-F", "#{pane_id} #{pane_pid}"])
            .unwrap()
            .stdout;
        let (pane, pid) = listing.trim().split_once(' ').unwrap();
        let (pane, pid) = (pane.to_owned(), pid.parse::<u32>().unwrap());

        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("flight"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        servers.allow_terminal(ServerId::new("flight"), endpoint);

        let node_id = Arc::new(Identity::generate().unwrap());
        let ui_id = Identity::generate().unwrap();
        let trust = trust_with(&[(&node_id, "mini-1")], &[(&ui_id, "laptop")]);
        let core = flight_orchestrator::OrchestratorConfig {
            terminal_lease_ttl_secs: ttl,
            ..Default::default()
        };
        let (server, orch, addr) = start_with_core(trust, None, core).await;
        let node_fp = node_id.fingerprint().clone();
        let link = node_link_stalling(&node_id, &addr, &orch, Arc::new(servers), stall);
        let (stop, stop_rx) = watch::channel(false);
        let runner = link.clone();
        let node_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        node_runtime.spawn(async move { runner.run(stop_rx).await });
        wait_until("node online", || {
            node_status(&server, &node_fp) == Some(NodeStatusCode::Online as i32)
        })
        .await;
        link.observe(vec![round(1, vec![obs(&pane, pid, PERMIT_SCREEN)])]);
        wait_until("pane replicated", || pane_count(&server, &node_fp) == 1).await;
        let mut ui = UiClient::connect(&addr, &ui_id, &orch).await.unwrap();
        ui.send(UiRequest {
            body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
        })
        .unwrap();
        ui_until(&mut ui, "snapshot", |e| {
            matches!(e.body, Some(ui_event_body::Body::Snapshot(_)))
        })
        .await;
        (
            Self {
                _serial: serial,
                server,
                addr,
                orch,
                ui_id,
                node_fp,
                tmux,
                tmux_name,
                stop,
                node_runtime: Some(node_runtime),
                ui,
                next_request: 100,
                lease_ttl: ttl,
                leases: std::sync::Mutex::new(Vec::new()),
            },
            pane,
            pid,
        )
    }

    /// Ask for a terminal; the id, or the error kind.
    async fn open(&mut self, pane: &str, pid: u32) -> Result<Vec<u8>, i32> {
        self.next_request += 1;
        let id = self.next_request;
        self.ui
            .send(UiRequest {
                body: Some(ui_request_body::Body::Command(Request {
                    request_id: id,
                    command: Some(Command {
                        kind: Some(ck::Kind::OpenTerminal(ck::OpenTerminal {
                            pane_ref: Some(PaneRefMsg {
                                host: self.node_fp.as_str().to_owned(),
                                server: "flight".into(),
                                pane: pane.into(),
                            }),
                            expected_pid: pid,
                            cols: 90,
                            rows: 25,
                            term: "xterm-256color".into(),
                            terminal_id: Vec::new(),
                        })),
                    }),
                })),
            })
            .unwrap();
        let answer = ui_until(
            &mut self.ui,
            "open answer",
            |e| matches!(&e.body, Some(ui_event_body::Body::Response(r)) if r.request_id == id),
        )
        .await;
        let Some(ui_event_body::Body::Response(Response { result, .. })) = answer.body else {
            unreachable!()
        };
        match result {
            Some(response_result::Result::Terminal(t)) => Ok(t.terminal_id),
            Some(response_result::Result::Error(e)) => Err(e.kind),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Attach as a UI whose presentation renews its lease three times per lifetime, until
    /// `stop_leasing`.
    async fn attach(&self, id: &[u8]) -> TerminalClient {
        let term = self.attach_unleased(id).await;
        let control = UiClient::connect(&self.addr, &self.ui_id, &self.orch)
            .await
            .expect("control");
        let (id, period) = (
            id.to_vec(),
            Duration::from_millis(self.lease_ttl * 1000 / 3),
        );
        let task = tokio::spawn(async move {
            loop {
                let lease = UiRequest {
                    body: Some(ui_request_body::Body::TerminalLease(TerminalLease {
                        terminal_id: id.clone(),
                    })),
                };
                if control.send(lease).is_err() {
                    return;
                }
                tokio::time::sleep(period).await;
            }
        });
        self.leases.lock().unwrap().push(task);
        term
    }

    /// Attach and never renew: a UI that is connected but not alive.
    async fn attach_unleased(&self, id: &[u8]) -> TerminalClient {
        TerminalClient::connect_ui(&self.addr, &self.ui_id, &self.orch, id)
            .await
            .expect("ui terminal")
    }

    fn stop_leasing(&self) {
        for task in self.leases.lock().unwrap().drain(..) {
            task.abort();
        }
    }

    /// Every resource of a finished terminal is released: the tmux client, the orchestrator's
    /// table entry and its relay.
    async fn assert_gone(&self) {
        self.wait_clients(0).await;
        wait_until("terminal table empty", || self.server.terminals_open() == 0).await;
        wait_until("relays released", || self.server.terminal_relays() == 0).await;
    }

    fn clients(&self) -> Vec<String> {
        self.tmux
            .runner()
            .run(&[
                "list-clients",
                "-F",
                "#{client_session} #{client_width}x#{client_height}",
            ])
            .map(|o| o.stdout.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    async fn wait_clients(&self, n: usize) {
        wait_until(&format!("{n} tmux clients"), || self.clients().len() == n).await;
    }

    fn tmux_server_pid(&self) -> u32 {
        self.tmux
            .runner()
            .run(&["display-message", "-p", "#{pid}"])
            .unwrap()
            .stdout
            .trim()
            .parse()
            .unwrap()
    }
}

impl Rig {
    /// The node disappears the way a killed process does: its tasks and sockets are dropped
    /// with no chance to say anything.
    fn vanish_node(&mut self) {
        if let Some(runtime) = self.node_runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.stop_leasing();
        self.vanish_node();
        let _ = self.tmux.kill_server();
        let _ = &self.tmux_name;
    }
}

fn rss_kib(pid: u32) -> u64 {
    Process::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

async fn read_until(term: &mut TerminalClient, needle: &str) -> String {
    let mut seen = String::new();
    within(&format!("terminal output {needle:?}"), async {
        while let Some(frame) = term.next().await.expect("terminal") {
            if let Some(terminal_body::Body::Data(d)) = frame.body {
                seen.push_str(&String::from_utf8_lossy(&d.payload));
                if seen.contains(needle) {
                    return;
                }
            }
        }
        panic!("terminal ended before {needle:?}: {seen:?}");
    })
    .await;
    seen
}

/// The reason in the terminal's final frame; drains output until it arrives.
async fn exit_reason(term: &mut TerminalClient) -> Option<i32> {
    within("terminal exit", async {
        loop {
            match term.next().await {
                Ok(Some(frame)) => {
                    if let Some(terminal_body::Body::Exit(e)) = frame.body {
                        return Some(e.reason);
                    }
                }
                Ok(None) | Err(_) => return None,
            }
        }
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_carries_keystrokes_output_and_size_to_the_pane_and_closes_cleanly() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("e2e", "cat").await;
    let id = rig.open(&pane, pid).await.expect("terminal opens");
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    assert_eq!(rig.clients(), vec!["work 90x25".to_owned()]);

    term.send(TerminalFrame::data(b"typed-over-flight\r".to_vec()))
        .await
        .unwrap();
    read_until(&mut term, "typed-over-flight").await;

    term.send(TerminalFrame {
        body: Some(terminal_body::Body::Resize(TerminalResize {
            cols: 70,
            rows: 20,
        })),
    })
    .await
    .unwrap();
    wait_until("resized", || rig.clients() == vec!["work 70x20".to_owned()]).await;

    term.send(TerminalFrame {
        body: Some(terminal_body::Body::Close(TerminalClose {})),
    })
    .await
    .unwrap();
    rig.wait_clients(0).await;
    wait_until("terminal forgotten", || rig.server.terminals_open() == 0).await;
    rig.tmux
        .runner()
        .run(&["has-session", "-t", "=work"])
        .expect("the session is untouched");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_pane_gets_pane_changed_and_no_terminal_exists() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("stale", "cat").await;
    // The UI looked at pid-1; the node publishes the real one.
    assert_eq!(
        rig.open(&pane, pid + 1).await,
        Err(ErrorKindCode::PaneChanged as i32)
    );
    assert_eq!(rig.server.terminals_open(), 0);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rig.clients().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_that_drops_its_end_leaves_no_client_and_no_entry() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("uidrop", "cat").await;
    let id = rig.open(&pane, pid).await.unwrap();
    let term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    drop(term);
    rig.wait_clients(0).await;
    wait_until("terminal forgotten", || rig.server.terminals_open() == 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn detaching_in_tmux_ends_the_terminal_with_a_reason() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("detach", "cat").await;
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    // A client is listed before it is attached to its session; detaching then does nothing.
    wait_until("attached to its session", || {
        rig.clients().iter().any(|c| c.starts_with("work "))
    })
    .await;
    rig.tmux
        .runner()
        .run(&["detach-client", "-s", "work"])
        .unwrap();
    assert_eq!(
        exit_reason(&mut term).await,
        Some(ExitReasonCode::ClientExited as i32)
    );
    wait_until("terminal forgotten", || rig.server.terminals_open() == 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_that_loses_its_link_ends_the_terminal_as_node_lost() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("nodelost", "cat").await;
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    rig.stop.send(true).unwrap();
    assert_eq!(
        exit_reason(&mut term).await,
        Some(ExitReasonCode::NodeLost as i32)
    );
    rig.wait_clients(0).await;
    wait_until("terminal forgotten", || rig.server.terminals_open() == 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_id_is_good_for_one_ui_attach_and_a_stranger_gets_nothing() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start("single", "cat").await;
    let id = rig.open(&pane, pid).await.unwrap();
    // A different, enrolled UI identity cannot use someone else's id.
    let other = Identity::generate().unwrap();
    let mut trust = rig.server.trust();
    trust.authorize(other.fingerprint(), "other", flight_trust::Role::Ui);
    rig.server
        .authorize(other.fingerprint(), "other", flight_trust::Role::Ui)
        .unwrap();
    let stolen = TerminalClient::connect_ui(&rig.addr, &other, &rig.orch, &id).await;
    let refused = match stolen {
        Ok(mut t) => matches!(t.next().await, Err(_) | Ok(None)),
        Err(_) => true,
    };
    assert!(refused, "another identity must not attach");
    let _first = rig.attach(&id).await;
    let second = TerminalClient::connect_ui(&rig.addr, &rig.ui_id, &rig.orch, &id).await;
    assert!(
        second.is_err(),
        "a second attach with the same id is refused"
    );
    drop(trust);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wedged_ui_costs_bounded_memory_everywhere_and_teardown_is_clean() {
    if !tmux_available() {
        return;
    }
    // A pane that never stops writing, a UI that stops reading.
    let (mut rig, pane, pid) = Rig::start(
        "wedge",
        "sh -c 'while :; do echo flood-flood-flood-flood-flood-flood; done'",
    )
    .await;
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    read_until(&mut term, "flood").await;

    let tmux_pid = rig.tmux_server_pid();
    let (tmux_before, mine_before) = (rss_kib(tmux_pid), rss_kib(std::process::id()));
    // Wedge: no reads for several seconds (shorter than the stall limit).
    rig.server.set_terminal_stall(Duration::from_secs(60));
    tokio::time::sleep(Duration::from_secs(5)).await;
    let (tmux_after, mine_after) = (rss_kib(tmux_pid), rss_kib(std::process::id()));
    let queued = rig.server.terminal_queue_peak();
    assert!(queued <= 4, "orchestrator queue peaked at {queued} frames");
    // This process holds the orchestrator, the node, its PTY threads and the UI end.
    assert!(
        mine_after < mine_before + 40_000,
        "process grew {} KiB while wedged",
        mine_after.saturating_sub(mine_before)
    );
    assert!(
        tmux_after < tmux_before + 40_000,
        "tmux grew {} KiB while wedged",
        tmux_after.saturating_sub(tmux_before)
    );
    // Not dead: reading again drains the stream, and because output was discarded while the
    // far end was behind, the screen is repainted behind a sequence-aborting prefix.
    read_until(&mut term, "\u{18}").await;
    read_until(&mut term, "flood").await;
    let _ = pane;

    term.send(TerminalFrame {
        body: Some(terminal_body::Body::Close(TerminalClose {})),
    })
    .await
    .unwrap();
    rig.wait_clients(0).await;
    wait_until("terminal forgotten", || rig.server.terminals_open() == 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_wedged_past_the_stall_limit_loses_the_terminal_and_nothing_is_left() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start_with(
        "stall",
        "sh -c 'while :; do echo flood-flood-flood-flood-flood-flood; done'",
        Duration::from_secs(2),
    )
    .await;
    // The orchestrator's own stall limit is the backstop behind the node's.
    rig.server.set_terminal_stall(Duration::from_secs(4));
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    read_until(&mut term, "flood").await;
    // Stop reading until the node gives up on a far end that never catches up.
    // The pipeline has to fill first (a few MB), then the node's limit, then the backstop.
    for _ in 0..600 {
        if rig.server.terminals_open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(rig.server.terminals_open(), 0, "the stall rule never fired");
    rig.wait_clients(0).await;
    drop(term);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_lost_while_the_relay_is_blocked_on_a_wedged_ui_is_noticed_promptly() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start_with(
        "lostblocked",
        "sh -c 'while :; do echo flood-flood-flood-flood-flood-flood; done'",
        Duration::from_secs(60),
    )
    .await;
    rig.server.set_terminal_stall(Duration::from_secs(60));
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    read_until(&mut term, "flood").await;
    // The UI stops reading; wait until the orchestrator's queue toward it is full.
    wait_until("the relay is blocked", || {
        rig.server.terminal_queue_peak() >= 4
    })
    .await;
    // Let the HTTP/2 windows in front of the queue fill as well.
    tokio::time::sleep(Duration::from_secs(8)).await;

    let lost = std::time::Instant::now();
    rig.vanish_node();
    for _ in 0..400 {
        if rig.server.terminals_open() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let took = lost.elapsed();
    eprintln!("node loss noticed after {took:?}");
    assert_eq!(
        rig.server.terminals_open(),
        0,
        "node loss was never noticed"
    );
    assert!(took < Duration::from_secs(5), "node loss took {took:?}");
    rig.wait_clients(0).await;
    drop(term);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_terminal_with_a_live_ui_outlives_many_lease_lifetimes() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start_full("idle", "cat", Duration::from_secs(30), 3).await;
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    // Four lifetimes of silence in both directions.
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert_eq!(
        rig.server.terminals_open(),
        1,
        "an idle terminal was torn down"
    );
    term.send(TerminalFrame::data(b"still-here\n".to_vec()))
        .await
        .unwrap();
    read_until(&mut term, "still-here").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_that_stops_renewing_loses_an_idle_terminal_and_nothing_is_left() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start_full("nolease", "cat", Duration::from_secs(30), 3).await;
    let id = rig.open(&pane, pid).await.unwrap();
    // Connected, reading, but never alive: no lease is ever sent.
    let mut term = rig.attach_unleased(&id).await;
    rig.wait_clients(1).await;
    let started = std::time::Instant::now();
    assert_eq!(
        exit_reason(&mut term).await,
        Some(ExitReasonCode::LeaseExpired as i32)
    );
    rig.assert_gone().await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(8), "took {took:?}");
    // The id is dead for good.
    let again = TerminalClient::connect_ui(&rig.addr, &rig.ui_id, &rig.orch, &id).await;
    let refused = match again {
        Ok(mut t) => matches!(t.next().await, Err(_) | Ok(None)),
        Err(_) => true,
    };
    assert!(refused, "an expired id must not attach");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ui_that_stops_renewing_and_stops_reading_loses_a_busy_terminal_too() {
    if !tmux_available() {
        return;
    }
    // Stall limits far away: only the lease can end this one.
    let (mut rig, pane, pid) = Rig::start_full(
        "busylease",
        "sh -c 'while :; do echo flood-flood-flood-flood-flood-flood; done'",
        Duration::from_secs(60),
        3,
    )
    .await;
    rig.server.set_terminal_stall(Duration::from_secs(60));
    let id = rig.open(&pane, pid).await.unwrap();
    let mut term = rig.attach(&id).await;
    rig.wait_clients(1).await;
    read_until(&mut term, "flood").await;
    wait_until("the relay is blocked", || {
        rig.server.terminal_queue_peak() >= 4
    })
    .await;
    rig.stop_leasing();
    let started = std::time::Instant::now();
    wait_until("the lease to lapse", || rig.server.terminals_open() == 0).await;
    rig.assert_gone().await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(10), "took {took:?}");
    drop(term);
}

#[tokio::test(flavor = "multi_thread")]
async fn fifty_terminals_come_and_go_and_leave_nothing_behind() {
    if !tmux_available() {
        return;
    }
    let (mut rig, pane, pid) = Rig::start_full("fifty", "cat", Duration::from_secs(30), 2).await;
    // More than the node's limit of four: a leaked slot would make a later open Busy.
    for n in 0..50 {
        let id = rig
            .open(&pane, pid)
            .await
            .unwrap_or_else(|k| panic!("open {n}: {k}"));
        let mut term = if n % 10 == 9 {
            // Every tenth UI just stops being alive.
            rig.attach_unleased(&id).await
        } else {
            rig.attach(&id).await
        };
        rig.wait_clients(1).await;
        if n % 10 == 9 {
            assert_eq!(
                exit_reason(&mut term).await,
                Some(ExitReasonCode::LeaseExpired as i32)
            );
        } else {
            term.send(TerminalFrame {
                body: Some(terminal_body::Body::Close(TerminalClose {})),
            })
            .await
            .unwrap();
        }
        rig.stop_leasing();
        rig.assert_gone().await;
    }
}
