// SPDX-License-Identifier: MIT

use super::*;
use crate::terminal::{LocalTerminal, TerminalEnd};
use flight_proto::ExitReasonCode;
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use flight_ui::SurfaceChoice::{self, Agent, Shell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// The far end of one attachment, as the test sees it.
struct Remote {
    choice: SurfaceChoice,
    from_session: mpsc::Receiver<ToRemote>,
    to_session: mpsc::Sender<FromRemote>,
}

enum Plan {
    Ok(Duration),
    Fail(OpenFailure, Duration),
}

struct FakeHost {
    plans: Mutex<VecDeque<Plan>>,
    attached: mpsc::UnboundedSender<Remote>,
    calls: Mutex<Vec<(SurfaceChoice, Option<Binding>)>>,
    renewals: Mutex<Vec<Vec<u8>>>,
    renew_fails: AtomicBool,
    /// Capacity of the channel toward the remote: a small one models a remote that is slow.
    capacity: usize,
}

fn binding(choice: SurfaceChoice, pid: u32) -> Binding {
    let pane = if choice == Agent { "%1" } else { "%2" };
    Binding {
        pane: PaneRef {
            host: HostId::new("h"),
            server: ServerId::new("s"),
            pane: PaneId::new(pane),
        },
        pid,
    }
}

impl FakeHost {
    fn new(capacity: usize) -> (Arc<Self>, mpsc::UnboundedReceiver<Remote>) {
        let (attached, rx) = mpsc::unbounded_channel();
        let host = Arc::new(Self {
            plans: Mutex::new(VecDeque::new()),
            attached,
            calls: Mutex::new(Vec::new()),
            renewals: Mutex::new(Vec::new()),
            renew_fails: AtomicBool::new(false),
            capacity,
        });
        (host, rx)
    }

    fn plan(&self, plan: Plan) {
        self.plans.lock().unwrap().push_back(plan);
    }

    async fn attach(&self, choice: SurfaceChoice) -> Result<Attachment, OpenFailure> {
        let plan = self.plans.lock().unwrap().pop_front();
        match plan {
            Some(Plan::Fail(failure, delay)) => {
                tokio::time::sleep(delay).await;
                return Err(failure);
            }
            Some(Plan::Ok(delay)) => tokio::time::sleep(delay).await,
            None => {}
        }
        let (to_remote, from_session) = mpsc::channel(self.capacity);
        let (to_session, from_remote) = mpsc::channel(4);
        let _ = self.attached.send(Remote {
            choice,
            from_session,
            to_session,
        });
        Ok(Attachment {
            id: vec![choice as u8 + 1],
            binding: binding(choice, 100),
            to_remote,
            from_remote,
            guard: None,
        })
    }
}

impl SurfaceHost for FakeHost {
    async fn open(&self, request: OpenRequest) -> Result<Attachment, OpenFailure> {
        self.calls
            .lock()
            .unwrap()
            .push((request.choice, request.expect));
        self.attach(request.choice).await
    }

    async fn connect(
        &self,
        _id: Vec<u8>,
        choice: SurfaceChoice,
    ) -> Result<Attachment, OpenFailure> {
        self.calls.lock().unwrap().push((choice, None));
        self.attach(choice).await
    }

    fn renew(&self, attachment: &[u8]) -> Result<(), String> {
        self.renewals.lock().unwrap().push(attachment.to_vec());
        if self.renew_fails.load(Ordering::Relaxed) {
            Err("gone".to_owned())
        } else {
            Ok(())
        }
    }
}

struct Rig {
    input: mpsc::Sender<Vec<u8>>,
    resizes: mpsc::Sender<(u16, u16)>,
    output: mpsc::Receiver<Vec<u8>>,
    notices: Arc<Mutex<Vec<String>>>,
    outcome: tokio::task::JoinHandle<SessionOutcome>,
    host: Arc<FakeHost>,
    remotes: mpsc::UnboundedReceiver<Remote>,
}

fn rig_with(capacity: usize, typed_ahead: &[u8], tweak: impl FnOnce(&mut SessionConfig)) -> Rig {
    let (host, remotes) = FakeHost::new(capacity);
    let (input, input_rx) = mpsc::channel(2);
    let (resizes, resizes_rx) = mpsc::channel(4);
    let (output_tx, output) = mpsc::channel(8);
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = notices.clone();
    let mut config = SessionConfig::new(Arc::new(move |s| sink.lock().unwrap().push(s.to_owned())));
    config.reset = b"<RESET>".to_vec();
    tweak(&mut config);
    let session = SurfaceSession::new(host.clone(), config);
    let start = SessionStart {
        id: vec![1],
        choice: Agent,
        typed_ahead: typed_ahead.to_vec(),
        size: (80, 24),
    };
    let outcome = tokio::spawn(session.run(
        LocalTerminal {
            input: input_rx,
            resizes: resizes_rx,
            output: output_tx,
        },
        start,
    ));
    Rig {
        input,
        resizes,
        output,
        notices,
        outcome,
        host,
        remotes,
    }
}

fn rig(typed_ahead: &[u8]) -> Rig {
    rig_with(4, typed_ahead, |_| {})
}

impl Rig {
    async fn type_(&self, bytes: &[u8]) {
        self.input.send(bytes.to_vec()).await.unwrap();
    }

    async fn next_remote(&mut self) -> Remote {
        tokio::time::timeout(Duration::from_secs(60), self.remotes.recv())
            .await
            .expect("an attachment")
            .expect("host alive")
    }

    async fn finish(self) -> SessionOutcome {
        tokio::time::timeout(Duration::from_secs(60), self.outcome)
            .await
            .expect("the session ends")
            .expect("no panic")
    }
}

impl Remote {
    /// Everything delivered until `want` bytes of data arrived.
    async fn data(&mut self, want: usize) -> Vec<u8> {
        let mut got = Vec::new();
        while got.len() < want {
            match tokio::time::timeout(Duration::from_secs(60), self.from_session.recv()).await {
                Ok(Some(ToRemote::Data(d))) => got.extend(d),
                Ok(Some(_)) => {}
                other => panic!("wanted {want} bytes, got {got:?} then {other:?}"),
            }
        }
        got
    }

    /// Whatever else is waiting right now.
    fn pending(&mut self) -> Vec<ToRemote> {
        let mut all = Vec::new();
        while let Ok(m) = self.from_session.try_recv() {
            all.push(m);
        }
        all
    }
}

#[tokio::test(start_paused = true)]
async fn keys_typed_while_the_first_surface_connects_arrive_in_order() {
    let mut rig = rig(b"ty");
    rig.host.plan(Plan::Ok(Duration::from_millis(300)));
    rig.type_(b"ped").await;
    rig.type_(b"-ahead\r").await;
    let mut agent = rig.next_remote().await;
    assert_eq!(agent.choice, Agent);
    assert_eq!(agent.data(12).await, b"typed-ahead\r".to_vec());
}

#[tokio::test(start_paused = true)]
async fn input_belongs_to_the_surface_chosen_when_it_was_typed_even_before_that_surface_is_up() {
    let mut rig = rig(b"");
    let mut agent = rig.next_remote().await;
    rig.host.plan(Plan::Ok(Duration::from_millis(500)));
    // Typed in one burst: some for the agent, then the switch, then for the shell, then back.
    rig.type_(b"for-agent\x00sfor-shell").await;
    rig.type_(b"\x00afor-agent-again").await;
    assert_eq!(agent.data(9).await, b"for-agent");
    let mut shell = rig.next_remote().await;
    assert_eq!(shell.choice, Shell);
    assert_eq!(shell.data(9).await, b"for-shell");
    // The agent still held the first attachment until the shell was up, then was let go; the
    // later switch back opens a new one.
    assert!(agent
        .pending()
        .iter()
        .all(|m| !matches!(m, ToRemote::Data(_))));
    let mut agent_again = rig.next_remote().await;
    assert_eq!(agent_again.choice, Agent);
    assert_eq!(agent_again.data(15).await, b"for-agent-again");
    assert!(shell
        .pending()
        .iter()
        .all(|m| !matches!(m, ToRemote::Data(_))));
    rig.type_(b"\x00q").await;
    let outcome = rig.finish().await;
    assert_eq!(outcome.end, TerminalEnd::UserLeft);
    assert_eq!(outcome.shown, Some(Agent));
    assert_eq!(outcome.undelivered, 0);
}

#[tokio::test(start_paused = true)]
async fn a_failed_switch_keeps_the_user_where_they_were_and_does_not_misdeliver() {
    let mut rig = rig(b"");
    let mut agent = rig.next_remote().await;
    rig.host.plan(Plan::Fail(
        OpenFailure::Refused("no shell".into()),
        Duration::from_millis(50),
    ));
    rig.type_(b"\x00styped-for-the-shell").await;
    // Give the failure time to happen, then type for the agent again.
    tokio::time::sleep(Duration::from_millis(200)).await;
    rig.type_(b"back-in-agent").await;
    assert_eq!(agent.data(13).await, b"back-in-agent");
    assert!(rig
        .notices
        .lock()
        .unwrap()
        .iter()
        .any(|n| n.contains("cannot show the shell: no shell")));
    rig.type_(b"\x00q").await;
    let outcome = rig.finish().await;
    assert_eq!(outcome.shown, Some(Agent));
    assert_eq!(outcome.undelivered, "typed-for-the-shell".len());
}

#[tokio::test(start_paused = true)]
async fn a_switch_is_atomic_the_old_surface_stays_until_the_new_one_is_up() {
    let mut rig = rig(b"");
    let mut agent = rig.next_remote().await;
    rig.host.plan(Plan::Ok(Duration::from_secs(2)));
    rig.type_(b"\x00s").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    // Still the agent: not closed, and its output still reaches the user.
    assert!(!agent.pending().iter().any(|m| matches!(m, ToRemote::Close)));
    agent
        .to_session
        .send(FromRemote::Data(b"agent-output".to_vec()))
        .await
        .unwrap();
    assert_eq!(rig.output.recv().await.unwrap(), b"agent-output");
    let _shell = rig.next_remote().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(agent.pending().iter().any(|m| matches!(m, ToRemote::Close)));
}

#[tokio::test(start_paused = true)]
async fn the_terminal_is_reset_between_surfaces_and_output_stays_ordered() {
    let mut rig = rig(b"");
    let agent = rig.next_remote().await;
    agent
        .to_session
        .send(FromRemote::Data(b"A1".to_vec()))
        .await
        .unwrap();
    assert_eq!(rig.output.recv().await.unwrap(), b"A1");
    rig.type_(b"\x00s").await;
    let shell = rig.next_remote().await;
    shell
        .to_session
        .send(FromRemote::Data(b"S1".to_vec()))
        .await
        .unwrap();
    assert_eq!(rig.output.recv().await.unwrap(), b"<RESET>");
    assert_eq!(rig.output.recv().await.unwrap(), b"S1");
    // A late word from the replaced attachment is never shown.
    let _ = agent
        .to_session
        .send(FromRemote::Data(b"A2".to_vec()))
        .await;
    shell
        .to_session
        .send(FromRemote::Data(b"S2".to_vec()))
        .await
        .unwrap();
    assert_eq!(rig.output.recv().await.unwrap(), b"S2");
}

#[tokio::test(start_paused = true)]
async fn the_end_of_a_replaced_attachment_is_ignored() {
    let mut rig = rig(b"");
    let agent = rig.next_remote().await;
    rig.type_(b"\x00s").await;
    let mut shell = rig.next_remote().await;
    let _ = agent
        .to_session
        .send(FromRemote::Exit {
            reason: ExitReasonCode::ClientExited,
            status: 0,
        })
        .await;
    rig.type_(b"x").await;
    assert_eq!(shell.data(1).await, b"x");
    assert!(!rig.outcome.is_finished());
}

#[tokio::test(start_paused = true)]
async fn the_input_queue_is_bounded_and_the_keyboard_is_not_read_beyond_it() {
    // A remote that never takes anything: capacity 1 and never read.
    let mut rig = rig_with(1, b"", |c| {
        c.input_limit = 1024;
        c.input_stall = Duration::from_secs(3);
    });
    let _agent = rig.next_remote().await;
    let mut accepted = 0usize;
    for _ in 0..200 {
        match tokio::time::timeout(Duration::from_millis(50), rig.input.send(vec![b'x'; 512])).await
        {
            Ok(Ok(())) => accepted += 512,
            _ => break,
        }
    }
    // One frame in the remote's channel, 1 KiB queued, one chunk of overshoot, and the two the
    // input channel itself holds: a small constant, not the 100 KiB offered.
    assert!(accepted <= 512 * 8, "accepted {accepted} bytes");
    assert!(accepted >= 1024);
    // A surface that stays wedged does not hold the keyboard forever: after the stall limit the
    // session ends and says why (the input queued for it is reported, not delivered elsewhere).
    let outcome = rig.finish().await;
    assert_eq!(
        outcome.end,
        TerminalEnd::Lost("the surface is not taking input".to_owned())
    );
    assert!(outcome.undelivered > 0 && outcome.undelivered <= 512 * 8);
}

#[tokio::test(start_paused = true)]
async fn leaving_works_while_the_surface_is_not_taking_input() {
    let mut rig = rig_with(1, b"", |c| c.input_limit = 1024);
    let _agent = rig.next_remote().await;
    rig.type_(b"stuck").await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    rig.type_(b"\x00q").await;
    let outcome = rig.finish().await;
    assert_eq!(outcome.end, TerminalEnd::UserLeft);
}

#[tokio::test(start_paused = true)]
async fn input_that_cannot_move_for_too_long_ends_the_session_with_the_reason() {
    let mut rig = rig_with(1, b"", |c| {
        c.input_limit = 8;
        c.input_stall = Duration::from_secs(5);
    });
    let _agent = rig.next_remote().await;
    for _ in 0..3 {
        rig.type_(b"0123456789abcdef").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let outcome = rig.finish().await;
    assert_eq!(
        outcome.end,
        TerminalEnd::Lost("the surface is not taking input".to_owned())
    );
    assert!(outcome.undelivered > 0);
}

#[tokio::test(start_paused = true)]
async fn a_lost_stream_is_attached_again_to_the_same_process_and_typing_waits() {
    let mut rig = rig(b"");
    let agent = rig.next_remote().await;
    rig.host.plan(Plan::Ok(Duration::from_millis(100)));
    agent
        .to_session
        .send(FromRemote::Lost("reset by peer".into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    rig.type_(b"during-the-outage").await;
    let mut again = rig.next_remote().await;
    assert_eq!(again.choice, Agent);
    assert_eq!(again.data(17).await, b"during-the-outage");
    let calls = rig.host.calls.lock().unwrap().clone();
    assert_eq!(
        calls.last().map(|c| c.1.clone()),
        Some(Some(binding(Agent, 100)))
    );
    assert!(rig
        .notices
        .lock()
        .unwrap()
        .iter()
        .any(|n| n.contains("trying again")));
}

#[tokio::test(start_paused = true)]
async fn reattaching_gives_up_after_the_configured_attempts_and_reports_undelivered_input() {
    let mut rig = rig_with(4, b"", |c| {
        c.reattach_delays = vec![Duration::from_millis(10), Duration::from_millis(10)];
    });
    let agent = rig.next_remote().await;
    for _ in 0..3 {
        rig.host.plan(Plan::Fail(
            OpenFailure::Unavailable("down".into()),
            Duration::ZERO,
        ));
    }
    agent
        .to_session
        .send(FromRemote::Lost("reset".into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    rig.type_(b"unsent").await;
    let outcome = rig.finish().await;
    assert_eq!(outcome.end, TerminalEnd::Lost("down".to_owned()));
    assert_eq!(outcome.undelivered, 6);
}

#[tokio::test(start_paused = true)]
async fn a_process_that_changed_while_reattaching_is_refused_and_input_is_not_delivered() {
    let mut rig = rig(b"");
    let agent = rig.next_remote().await;
    rig.host.plan(Plan::Fail(
        OpenFailure::Refused("the pane changed".into()),
        Duration::ZERO,
    ));
    agent
        .to_session
        .send(FromRemote::Lost("reset".into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1)).await;
    rig.type_(b"secret").await;
    let outcome = rig.finish().await;
    assert_eq!(
        outcome.end,
        TerminalEnd::Lost("the pane changed".to_owned())
    );
    assert_eq!(outcome.undelivered, 6);
}

#[tokio::test(start_paused = true)]
async fn the_node_ending_the_terminal_ends_the_session_with_its_reason() {
    let mut rig = rig(b"");
    let agent = rig.next_remote().await;
    agent
        .to_session
        .send(FromRemote::Exit {
            reason: ExitReasonCode::ClientExited,
            status: 0,
        })
        .await
        .unwrap();
    let outcome = rig.finish().await;
    assert_eq!(
        outcome.end,
        TerminalEnd::Exited {
            reason: ExitReasonCode::ClientExited,
            status: 0
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_resize_during_a_switch_is_sent_right_after_attach_and_later_ones_follow() {
    let mut rig = rig(b"");
    let _agent = rig.next_remote().await;
    rig.host.plan(Plan::Ok(Duration::from_millis(500)));
    rig.type_(b"\x00s").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    rig.resizes.send((120, 40)).await.unwrap();
    let mut shell = rig.next_remote().await;
    // The shell was asked for at 80x24; it hears of the change as soon as it is up.
    let first = tokio::time::timeout(Duration::from_secs(5), shell.from_session.recv()).await;
    assert!(
        matches!(first, Ok(Some(ToRemote::Resize(120, 40)))),
        "{first:?}"
    );
    rig.resizes.send((100, 30)).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(5), shell.from_session.recv()).await;
    assert!(
        matches!(next, Ok(Some(ToRemote::Resize(100, 30)))),
        "{next:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_lease_is_renewed_for_the_attachment_on_screen_and_a_dead_link_ends_the_session() {
    let mut rig = rig_with(4, b"", |c| c.lease_period = Duration::from_secs(5));
    let _agent = rig.next_remote().await;
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert!(rig
        .host
        .renewals
        .lock()
        .unwrap()
        .iter()
        .all(|id| id == &vec![Agent as u8 + 1]));
    assert!(rig.host.renewals.lock().unwrap().len() >= 2);
    rig.host.renew_fails.store(true, Ordering::Relaxed);
    let outcome = rig.finish().await;
    assert!(
        matches!(outcome.end, TerminalEnd::Lost(ref why) if why.contains("control connection"))
    );
}
