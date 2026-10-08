// SPDX-License-Identifier: MIT

//! The node's end of a terminal: bridge a PTY running a tmux client to the orchestrator's
//! terminal stream. The PTY side is blocking and runs on its own threads; the stream side is
//! async. Both hand-offs are bounded, so a slow reader slows the PTY instead of growing memory.

use crate::terminal_client::TerminalClient;
use flight_node::OpenedTerminal;
use flight_proto::{
    terminal_body, ExitReasonCode, Origin, TerminalExit, TerminalFrame, MAX_TERMINAL_DATA,
};
use flight_trust::{Fingerprint, Identity};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify, OwnedSemaphorePermit};

/// Commands to the thread that owns the PTY's input side.
enum Command {
    Write(Vec<u8>),
    Resize(u16, u16),
}

/// How long the node waits for the orchestrator to accept its terminal stream.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
/// Output waiting between the PTY reader and the stream. Beyond this the far end is behind
/// and output is discarded.
const OUTPUT_CAP: usize = 64 * 1024;
/// The least time between two output frames.
const MIN_FRAME_GAP: Duration = Duration::from_millis(8);
/// Sent before a redraw: CAN and SUB abort an escape sequence the terminal may be in the
/// middle of, then reset attributes.
const RESYNC_PREFIX: &[u8] = b"\x18\x1a\x1b[0m";
/// How long a hung-up tmux client has to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct NodeTerminalEnd {
    pub(crate) address: String,
    pub(crate) identity: Arc<Identity>,
    pub(crate) orchestrator: Fingerprint,
    pub(crate) dial_timeout: Duration,
    /// How long the far end may stay behind before the terminal is given up.
    pub(crate) stall: Duration,
}

/// Run one terminal to its end. Holds a slot of the node's terminal limit until then.
pub(crate) async fn run(
    end: NodeTerminalEnd,
    terminal_id: [u8; 16],
    opened: OpenedTerminal,
    _slot: OwnedSemaphorePermit,
    log: impl Fn(String) + Send + Sync + 'static,
) {
    let OpenedTerminal {
        process,
        reader,
        redraw,
    } = opened;
    let killer = process.hang_up_handle();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(4);
    let (code_tx, code_rx) = oneshot::channel::<Option<i32>>();
    let writer = std::thread::spawn(move || write_loop(process, cmd_rx, code_tx));

    let hang_up = || killer.clone().hang_up();

    let attach = tokio::time::timeout(
        ATTACH_TIMEOUT,
        TerminalClient::connect(
            &end.address,
            &end.identity,
            &end.orchestrator,
            &terminal_id,
            Origin::Node,
            end.dial_timeout,
        ),
    )
    .await;
    let mut client = match attach {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => {
            log(format!("terminal: cannot attach to the orchestrator: {e}"));
            hang_up();
            drop(cmd_tx);
            let _ = writer.join();
            return;
        }
        Err(_) => {
            log("terminal: the orchestrator did not accept the stream in time".to_owned());
            hang_up();
            drop(cmd_tx);
            let _ = writer.join();
            return;
        }
    };

    // Output: the blocking reader appends to a bounded buffer and this task sends it on in
    // frames of up to 16 KiB. Reads that arrive while a send is waiting coalesce into larger
    // frames instead of queueing as many small ones.
    let output = Arc::new(Output::default());
    let discarded = Arc::new(AtomicU64::new(0));
    let reader_thread = {
        let (output, discarded) = (output.clone(), discarded.clone());
        std::thread::spawn(move || read_loop(reader, &output, redraw, &discarded))
    };

    let mut reason = ExitReasonCode::ClientExited;
    let mut hung_up_by_client = false;
    let mut last_send = std::time::Instant::now() - MIN_FRAME_GAP;
    loop {
        tokio::select! {
            _ = output.wake.notified() => {
                loop {
                    // At most ~125 frames a second: a flood is sent in large frames, and
                    // typing is still sent at once. Many tiny frames would trip HTTP/2's
                    // protection against floods of small DATA frames on a slow reader.
                    let since = last_send.elapsed();
                    if since < MIN_FRAME_GAP {
                        tokio::time::sleep(MIN_FRAME_GAP - since).await;
                    }
                    let (bytes, done) = output.take();
                    if bytes.is_empty() {
                        if done {
                            // The tmux client has gone and everything it said was sent.
                            hung_up_by_client = true;
                        }
                        break;
                    }
                    // A send that cannot complete for the whole stall limit means the far end
                    // is not taking anything: give the terminal up.
                    match tokio::time::timeout(end.stall, client.send(TerminalFrame::data(bytes))).await {
                        Ok(Ok(())) => last_send = std::time::Instant::now(),
                        Ok(Err(_)) => {
                            reason = ExitReasonCode::NodeLost;
                            break;
                        }
                        Err(_) => {
                            reason = ExitReasonCode::Stalled;
                            break;
                        }
                    }
                }
                if hung_up_by_client || reason != ExitReasonCode::ClientExited {
                    break;
                }
            },
            frame = client.next() => match frame {
                Ok(Some(frame)) => match frame.body {
                    Some(terminal_body::Body::Data(d)) => {
                        if cmd_tx.send(Command::Write(d.payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(terminal_body::Body::Resize(r)) => {
                        let dim = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
                        let _ = cmd_tx.send(Command::Resize(dim(r.cols), dim(r.rows))).await;
                    }
                    Some(terminal_body::Body::Close(_)) => {
                        reason = ExitReasonCode::ClosedByUi;
                        break;
                    }
                    _ => {}
                },
                // The stream ended without a close: the orchestrator or the UI is gone.
                Ok(None) | Err(_) => {
                    reason = ExitReasonCode::ClosedByUi;
                    break;
                }
            },
        }
    }

    hang_up();
    drop(cmd_tx);
    let status = tokio::time::timeout(REAP_GRACE * 2, code_rx)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
    if matches!(
        reason,
        ExitReasonCode::ClientExited | ExitReasonCode::Stalled
    ) {
        let exit = TerminalFrame {
            body: Some(terminal_body::Body::Exit(TerminalExit {
                reason: reason as i32,
                status: status.unwrap_or(0),
            })),
        };
        // A stalled far end will not read a goodbye: do not wait long for it.
        let stalled_far_end = reason == ExitReasonCode::Stalled;
        let patience = Duration::from_millis(if stalled_far_end { 300 } else { 2000 });
        let _ = tokio::time::timeout(patience, client.send(exit)).await;
        if stalled_far_end {
            drop(client);
        } else {
            client.finish(Duration::from_secs(2)).await;
        }
    } else {
        drop(client);
    }
    let _ = reader_thread.join();
    let _ = writer.join();
    log(format!(
        "terminal ended: {reason:?} ({} KiB of output discarded while the far end was behind)",
        discarded.load(Ordering::Relaxed) / 1024
    ));
}

/// The bytes a terminal's tmux client wrote, waiting to be sent.
#[derive(Default)]
struct Output {
    state: Mutex<OutputState>,
    wake: Notify,
}

#[derive(Default)]
struct OutputState {
    buf: Vec<u8>,
    /// Output was discarded because the far end did not keep up; a repaint is owed.
    behind: bool,
    /// The tmux client has gone (end of file).
    done: bool,
}

impl Output {
    fn lock(&self) -> std::sync::MutexGuard<'_, OutputState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Up to one frame's worth of waiting output, and whether the client has gone.
    fn take(&self) -> (Vec<u8>, bool) {
        let mut st = self.lock();
        let n = st.buf.len().min(MAX_TERMINAL_DATA);
        let bytes: Vec<u8> = st.buf.drain(..n).collect();
        (bytes, st.done)
    }
}

/// Blocking: PTY output into the buffer.
///
/// The tmux client must never be left blocked on its terminal: tmux buffers a slow client's
/// output in the server without bound. So this thread always keeps reading. When the far end
/// is behind (the buffer is full) the output is discarded, and once the buffer has drained the
/// screen is repainted: a prefix that aborts any half-received escape sequence, then a tmux
/// redraw of the whole client.
fn read_loop(
    mut reader: Box<dyn Read + Send>,
    output: &Output,
    redraw: flight_node::Redraw,
    discarded: &AtomicU64,
) {
    let mut buf = vec![0u8; MAX_TERMINAL_DATA];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let Some(chunk) = buf.get(..n) else { break };
        let mut st = output.lock();
        if st.behind {
            discarded.fetch_add(n as u64, Ordering::Relaxed);
            if st.buf.is_empty() {
                // The far end caught up. What was just read belongs to a stream with a hole
                // in it; the redraw that follows replaces it.
                st.behind = false;
                st.buf.extend_from_slice(RESYNC_PREFIX);
                drop(st);
                output.wake.notify_one();
                redraw();
            }
            continue;
        }
        if st.buf.len() + n > OUTPUT_CAP {
            st.behind = true;
            discarded.fetch_add(n as u64, Ordering::Relaxed);
            continue;
        }
        st.buf.extend_from_slice(chunk);
        drop(st);
        output.wake.notify_one();
    }
    output.lock().done = true;
    output.wake.notify_one();
}

/// Blocking: PTY input and resizes. Owns the process; dropping it reaps the tmux client.
fn write_loop(
    mut process: flight_node::TerminalProcess,
    mut commands: mpsc::Receiver<Command>,
    done: oneshot::Sender<Option<i32>>,
) {
    while let Some(command) = commands.blocking_recv() {
        let ok = match command {
            Command::Write(bytes) => process.write_all(&bytes).is_ok(),
            Command::Resize(cols, rows) => process.resize(cols, rows).is_ok(),
        };
        if !ok {
            break;
        }
    }
    let _ = done.send(process.hang_up(REAP_GRACE));
}
