// SPDX-License-Identifier: MIT

use super::*;
use crate::screens::{Geometry, ScreenModel};
use crate::session::{
    Attachment, Binding, FromRemote, OpenFailure, OpenRequest, SurfaceHost, ToRemote,
};
use crate::terminal::{LocalTerminal, TerminalEnd};
use flight_present::{Axis, Layout, Placement};
use flight_proto::ExitReasonCode;
use flight_state::{HostId, PaneId, PaneRef, ServerId, SurfaceId};
use flight_ui::SurfaceChoice::{self, Agent, Shell};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// The far end of one attachment, as the test sees it.
struct Remote {
    choice: SurfaceChoice,
    expect: Option<Binding>,
    size: (u16, u16),
    from_session: mpsc::Receiver<ToRemote>,
    to_session: mpsc::Sender<FromRemote>,
}

struct FakeHost {
    attached: mpsc::UnboundedSender<Remote>,
    refuse: Mutex<Vec<SurfaceChoice>>,
}

fn binding(choice: SurfaceChoice) -> Binding {
    Binding {
        pane: PaneRef {
            host: HostId::new("h"),
            server: ServerId::new("s"),
            pane: PaneId::new(if choice == Agent { "%1" } else { "%2" }),
        },
        pid: 100,
    }
}

impl SurfaceHost for FakeHost {
    async fn open(&self, request: OpenRequest) -> Result<Attachment, OpenFailure> {
        if self.refuse.lock().unwrap().contains(&request.choice) {
            return Err(OpenFailure::Refused("gone".to_owned()));
        }
        let (to_remote, from_session) = mpsc::channel(16);
        let (to_session, from_remote) = mpsc::channel(16);
        let _ = self.attached.send(Remote {
            choice: request.choice,
            expect: request.expect,
            size: (request.cols, request.rows),
            from_session,
            to_session,
        });
        Ok(Attachment {
            id: vec![request.choice as u8 + 1],
            binding: binding(request.choice),
            to_remote,
            from_remote,
            guard: None,
            retired: None,
        })
    }

    async fn connect(
        &self,
        _id: Vec<u8>,
        choice: SurfaceChoice,
        _binding: Binding,
    ) -> Result<Attachment, OpenFailure> {
        self.open(OpenRequest {
            choice,
            cols: 80,
            rows: 24,
            expect: None,
        })
        .await
    }

    fn renew(&self, _attachment: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

struct Rig {
    input: mpsc::Sender<Vec<u8>>,
    resizes: mpsc::Sender<(u16, u16)>,
    screen: ScreenModel,
    output: mpsc::Receiver<Vec<u8>>,
    notices: Arc<Mutex<Vec<String>>>,
    outcome: tokio::task::JoinHandle<PresentationOutcome>,
    host: Arc<FakeHost>,
    remotes: mpsc::UnboundedReceiver<Remote>,
}

fn id(s: &str) -> SurfaceId {
    SurfaceId::new(s)
}

fn side_by_side() -> Layout {
    Layout::single(id("agent"))
        .split(&id("agent"), Axis::Across, id("shell"), Placement::After)
        .unwrap()
        .focus_on(&id("agent"))
        .unwrap()
}

fn start(layout: Layout, size: (u16, u16)) -> Rig {
    let (attached, remotes) = mpsc::unbounded_channel();
    let host = Arc::new(FakeHost {
        attached,
        refuse: Mutex::new(Vec::new()),
    });
    let notices = Arc::new(Mutex::new(Vec::new()));
    let said = notices.clone();
    let config = PresentationConfig::for_workspace(Arc::new(move |t| {
        said.lock().unwrap().push(t.to_owned());
    }));
    let (input, input_rx) = mpsc::channel(16);
    let (resizes, resizes_rx) = mpsc::channel(4);
    let (output_tx, output) = mpsc::channel(64);
    let session = PresentationSession::new(host.clone(), config);
    let outcome = tokio::spawn(session.run(
        LocalTerminal {
            input: input_rx,
            resizes: resizes_rx,
            output: output_tx,
        },
        layout,
        size,
    ));
    Rig {
        input,
        resizes,
        screen: ScreenModel::new(Geometry::new(size.0, size.1).unwrap_or(Geometry::STANDARD)),
        output,
        notices,
        outcome,
        host,
        remotes,
    }
}

impl Rig {
    async fn remote(&mut self) -> Remote {
        tokio::time::timeout(Duration::from_secs(5), self.remotes.recv())
            .await
            .expect("an attachment was made")
            .expect("the host is alive")
    }

    /// Let everything the session has to say reach `screen`.
    async fn settle(&mut self) {
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(bytes) = self.output.try_recv() {
            self.screen.feed(&bytes);
        }
    }

    async fn type_(&mut self, bytes: &[u8]) {
        self.input.send(bytes.to_vec()).await.unwrap();
        self.settle().await;
    }
}

async fn received(remote: &mut Remote) -> Vec<ToRemote> {
    let mut got = Vec::new();
    while let Ok(m) = remote.from_session.try_recv() {
        got.push(m);
    }
    got
}

fn data(parts: &[ToRemote]) -> Vec<u8> {
    parts
        .iter()
        .filter_map(|m| match m {
            ToRemote::Data(d) => Some(d.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[tokio::test(start_paused = true)]
async fn both_surfaces_are_attached_at_their_own_tile_size_and_drawn_together() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (agent, shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    // 81 columns: 40 + a line + 40.
    assert_eq!(agent.size, (40, 24));
    assert_eq!(shell.size, (40, 24));
    agent
        .to_session
        .send(FromRemote::Data(b"agent here".to_vec()))
        .await
        .unwrap();
    shell
        .to_session
        .send(FromRemote::Data(b"shell here".to_vec()))
        .await
        .unwrap();
    rig.settle().await;
    let row = rig.screen.row_text(0);
    assert!(row.contains("agent here"), "{row:?}");
    assert!(row.contains("shell here"), "{row:?}");
    assert!(row.find("agent here") < row.find("shell here"));
}

#[tokio::test(start_paused = true)]
async fn the_keyboard_belongs_to_the_focused_surface_and_keeps_its_order_across_a_focus_change() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (mut agent, mut shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    rig.type_(b"one").await;
    rig.type_(b"\x00ltwo").await;
    rig.type_(b"\x00hthree").await;
    assert_eq!(data(&received(&mut agent).await), b"onethree");
    assert_eq!(data(&received(&mut shell).await), b"two");
}

#[tokio::test(start_paused = true)]
async fn a_terminal_resize_reaches_each_attachment_as_its_own_tile_size() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (mut agent, mut shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    rig.resizes.send((121, 40)).await.unwrap();
    rig.settle().await;
    for remote in [&mut agent, &mut shell] {
        let sizes: Vec<_> = received(remote)
            .await
            .into_iter()
            .filter_map(|m| match m {
                ToRemote::Resize(c, r) => Some((c, r)),
                _ => None,
            })
            .collect();
        assert_eq!(sizes.last(), Some(&(60, 40)), "{sizes:?}");
    }
}

fn resizes_of(parts: &[ToRemote]) -> Vec<(u16, u16)> {
    parts
        .iter()
        .filter_map(|m| match m {
            ToRemote::Resize(c, r) => Some((*c, *r)),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_terminal_squeezed_to_nothing_keeps_every_surface_at_its_last_size_and_alive() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (mut agent, mut shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    received(&mut agent).await;
    received(&mut shell).await;
    for size in [(1, 1), (0, 0), (1, 30), (30, 1), (0, 5), (1, 1)] {
        rig.resizes.send(size).await.unwrap();
        rig.settle().await;
    }
    for remote in [&mut agent, &mut shell] {
        let got = received(remote).await;
        assert!(!got.contains(&ToRemote::Close), "{got:?}");
        // The surface is told no size at all while the viewport cannot hold a screen.
        assert!(resizes_of(&got).is_empty(), "{got:?}");
    }
    // The session is still running, and the keyboard still reaches the focused surface.
    assert!(!rig.outcome.is_finished());
    rig.type_(b"still here").await;
    assert_eq!(data(&received(&mut agent).await), b"still here");
    // Room again: each surface hears its tile's size.
    rig.resizes.send((121, 40)).await.unwrap();
    rig.settle().await;
    for remote in [&mut agent, &mut shell] {
        assert_eq!(resizes_of(&received(remote).await).last(), Some(&(60, 40)));
    }
}

#[tokio::test(start_paused = true)]
async fn rapid_resizes_end_with_each_surface_told_the_size_of_the_final_tile() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (mut agent, mut shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    let (mut told_agent, mut told_shell) = (Vec::new(), Vec::new());
    for i in 0u16..40 {
        let size = if i % 5 == 0 {
            (1, 1)
        } else {
            (41 + i * 3, 5 + i % 17)
        };
        rig.resizes.send(size).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        told_agent.extend(resizes_of(&received(&mut agent).await));
        told_shell.extend(resizes_of(&received(&mut shell).await));
    }
    rig.resizes.send((101, 30)).await.unwrap();
    rig.settle().await;
    told_agent.extend(resizes_of(&received(&mut agent).await));
    told_shell.extend(resizes_of(&received(&mut shell).await));
    for sizes in [told_agent, told_shell] {
        assert_eq!(sizes.last(), Some(&(50, 30)), "{sizes:?}");
        // Whatever was sent in between was a size a screen can have.
        assert!(sizes.iter().all(|&(c, r)| c >= 2 && r >= 2), "{sizes:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_terminal_that_starts_too_small_attaches_the_focused_surface_at_the_standard_size_then_follows(
) {
    let mut rig = start(side_by_side(), (1, 1));
    let mut agent = rig.remote().await;
    assert_eq!(agent.choice, Agent);
    assert_eq!(agent.size, (80, 24));
    rig.resizes.send((81, 24)).await.unwrap();
    rig.settle().await;
    assert_eq!(
        resizes_of(&received(&mut agent).await).last(),
        Some(&(40, 24))
    );
    assert_eq!(rig.remote().await.size, (40, 24));
}

#[tokio::test(start_paused = true)]
async fn closing_a_tile_lets_the_surface_go_and_leaves_the_other_running_and_a_split_brings_it_back(
) {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (mut agent, mut shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    rig.type_(b"\x00x").await; // closes the agent's tile
    assert!(received(&mut agent).await.contains(&ToRemote::Close));
    // The shell is untouched except that it now fills the terminal.
    let shell_got = received(&mut shell).await;
    assert!(!shell_got.contains(&ToRemote::Close));
    assert!(
        shell_got.contains(&ToRemote::Resize(81, 24)),
        "{shell_got:?}"
    );
    // Showing it again attaches it again, as a new attachment, with the shell untouched.
    rig.type_(b"\x00|").await;
    let again = rig.remote().await;
    assert_eq!(again.choice, Agent);
    assert_eq!(again.size, (40, 24));
}

#[tokio::test(start_paused = true)]
async fn a_surface_in_a_hidden_tab_is_not_attached_until_its_tab_shows() {
    let layout = Layout::single(id("agent"))
        .add_tab(&id("agent"), id("shell"))
        .unwrap()
        .focus_on(&id("agent"))
        .unwrap();
    let mut rig = start(layout, (80, 24));
    let mut agent = rig.remote().await;
    rig.settle().await;
    assert!(
        rig.remotes.try_recv().is_err(),
        "the shell is in a tab that is not showing"
    );
    rig.type_(b"\x00n").await;
    let shell = rig.remote().await;
    assert_eq!(shell.choice, Shell);
    assert!(received(&mut agent).await.contains(&ToRemote::Close));
}

#[tokio::test(start_paused = true)]
async fn a_surface_that_exits_is_named_in_its_tile_and_the_session_ends_when_all_have() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (agent, shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    agent
        .to_session
        .send(FromRemote::Exit {
            reason: ExitReasonCode::ClientExited,
            status: 0,
        })
        .await
        .unwrap();
    shell
        .to_session
        .send(FromRemote::Data(b"still going".to_vec()))
        .await
        .unwrap();
    rig.settle().await;
    assert!(!rig.outcome.is_finished());
    assert!(rig.screen.row_text(0).contains("still going"));
    assert!(rig.screen.row_text(0).contains("tmux client ended"));
    shell
        .to_session
        .send(FromRemote::Exit {
            reason: ExitReasonCode::ClientExited,
            status: 0,
        })
        .await
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(5), rig.outcome)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(outcome.end, TerminalEnd::Exited { .. }));
}

#[tokio::test(start_paused = true)]
async fn a_broken_stream_is_attached_again_to_the_same_process_and_input_waits_for_it() {
    let mut rig = start(side_by_side(), (81, 24));
    let (a, s) = (rig.remote().await, rig.remote().await);
    let (agent, _shell) = if a.choice == Agent { (a, s) } else { (s, a) };
    agent
        .to_session
        .send(FromRemote::Lost("reset".to_owned()))
        .await
        .unwrap();
    rig.settle().await;
    rig.type_(b"typed while down").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut again = rig.remote().await;
    assert_eq!(again.choice, Agent);
    assert_eq!(
        again.expect,
        Some(binding(Agent)),
        "only the same process will do"
    );
    rig.settle().await;
    assert_eq!(data(&received(&mut again).await), b"typed while down");
}

#[tokio::test(start_paused = true)]
async fn input_for_a_surface_that_cannot_be_attached_is_counted_not_sent_elsewhere() {
    let layout = side_by_side();
    let mut rig = start(layout, (81, 24));
    rig.host.refuse.lock().unwrap().push(Shell);
    let _agent = rig.remote().await;
    rig.settle().await;
    rig.type_(b"\x00l").await;
    rig.type_(b"lost").await;
    rig.type_(b"\x00q").await;
    let outcome = rig.outcome.await.unwrap();
    assert_eq!(outcome.end, TerminalEnd::UserLeft);
    assert!(outcome.undelivered <= 4);
}

#[tokio::test(start_paused = true)]
async fn leaving_returns_the_layout_as_the_user_left_it() {
    let mut rig = start(Layout::single(id("agent")), (80, 24));
    let _agent = rig.remote().await;
    rig.type_(b"\x00-").await;
    let _shell = rig.remote().await;
    rig.type_(b"\x00q").await;
    let outcome = rig.outcome.await.unwrap();
    assert_eq!(outcome.layout.surfaces().len(), 2);
    assert_eq!(outcome.layout.focus(), &id("shell"));
    assert!(rig.notices.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_split_with_every_surface_already_shown_says_so_and_changes_nothing() {
    let mut rig = start(side_by_side(), (81, 24));
    let _ = (rig.remote().await, rig.remote().await);
    rig.type_(b"\x00|").await;
    assert!(rig
        .notices
        .lock()
        .unwrap()
        .iter()
        .any(|n| n.contains("already shown")));
    assert!(rig.remotes.try_recv().is_err());
}
