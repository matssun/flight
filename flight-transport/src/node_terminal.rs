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
/// How long after the tmux client exits the reader gets to deliver its last output.
const EXIT_DRAIN: Duration = Duration::from_millis(150);
/// How often the input side looks for work and for the client having exited.
const WRITER_POLL: Duration = Duration::from_millis(10);
/// How long a hung-up tmux client has to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(2);
/// After the UI says it is done, how long the tmux client is left alone before it is hung up,
/// so that input written just before the goodbye (a key, then a switch to another surface) is
/// read by the client instead of being discarded with it.
const CLOSE_SETTLE: Duration = Duration::from_millis(100);
/// After the orchestrator stops taking output, how long the node keeps reading what it had
/// already sent.
const INBOUND_DRAIN: Duration = Duration::from_secs(2);

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
        cleanup,
    } = opened;
    // Whatever the terminal made besides its process goes at every exit, after the process.
    let _cleanup = CleanupOnDrop(Some(cleanup));
    let killer = process.hang_up_handle();
    let output = Arc::new(Output::default());
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(4);
    let (code_tx, code_rx) = oneshot::channel::<Option<i32>>();
    let writer = {
        let output = output.clone();
        std::thread::spawn(move || write_loop(process, cmd_rx, &output, code_tx))
    };

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
    let discarded = Arc::new(AtomicU64::new(0));
    let reader_thread = {
        let (output, discarded) = (output.clone(), discarded.clone());
        std::thread::spawn(move || read_loop(reader, &output, redraw, &discarded))
    };

    let mut reason = ExitReasonCode::ClientExited;
    let mut hung_up_by_client = false;
    // The UI said goodbye, as opposed to vanishing: its last input is let through first.
    let mut said_goodbye = false;
    // Output goes to the orchestrator until a send fails. After that the loop still reads what
    // the orchestrator had already sent (the user's last keys, the goodbye), for a short time,
    // before it ends: the far end closing its side is not a reason to discard what is queued.
    let mut sending = true;
    let mut drain_until: Option<tokio::time::Instant> = None;
    let mut last_send = std::time::Instant::now() - MIN_FRAME_GAP;
    loop {
        tokio::select! {
            () = async {
                match drain_until {
                    Some(until) => tokio::time::sleep_until(until).await,
                    None => std::future::pending().await,
                }
            } => break,
            _ = output.wake.notified(), if sending => {
                // One frame, then back to the select: whatever the user sent meanwhile (a
                // Ctrl-C to a program flooding the terminal) is read between frames, not after
                // the flood.
                //
                // At most ~125 frames a second: a flood is sent in large frames, and typing is
                // still sent at once. Many tiny frames would trip HTTP/2's protection against
                // floods of small DATA frames on a slow reader.
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
                } else {
                    // A send that cannot complete for the whole stall limit means the far end
                    // is not taking anything: give the terminal up.
                    match tokio::time::timeout(end.stall, client.send(TerminalFrame::data(bytes))).await {
                        Ok(Ok(())) => last_send = std::time::Instant::now(),
                        Ok(Err(_)) => {
                            reason = ExitReasonCode::NodeLost;
                            sending = false;
                            let now = tokio::time::Instant::now();
                            drain_until = Some(now.checked_add(INBOUND_DRAIN).unwrap_or(now));
                        }
                        Err(_) => reason = ExitReasonCode::Stalled,
                    }
                }
                if hung_up_by_client || (sending && reason != ExitReasonCode::ClientExited) {
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
                        said_goodbye = true;
                        break;
                    }
                    _ => {}
                },
                // The stream ended without a close: the orchestrator or the UI is gone.
                Ok(None) | Err(_) => {
                    if reason == ExitReasonCode::ClientExited {
                        reason = ExitReasonCode::ClosedByUi;
                    }
                    break;
                }
            },
        }
    }

    if said_goodbye {
        // The writer sends what is queued, waits a moment, and hangs the client up itself.
        drop(cmd_tx);
    } else {
        hang_up();
        drop(cmd_tx);
    }
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

/// Runs a terminal's cleanup when the terminal is over, on a thread of its own so that tmux
/// being slow never holds the runtime.
struct CleanupOnDrop(Option<flight_node::Redraw>);

impl Drop for CleanupOnDrop {
    fn drop(&mut self) {
        if let Some(cleanup) = self.0.take() {
            std::thread::spawn(cleanup);
        }
    }
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
        // More waiting, or the end to be noticed: come round again.
        if !st.buf.is_empty() || st.done {
            self.wake.notify_one();
        }
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
    output: &Output,
    done: oneshot::Sender<Option<i32>>,
) {
    let mut exited_at: Option<std::time::Instant> = None;
    loop {
        match commands.try_recv() {
            Ok(command) => {
                let ok = match command {
                    Command::Write(bytes) => process.write_all(&bytes).is_ok(),
                    Command::Resize(cols, rows) => process.resize(cols, rows).is_ok(),
                };
                if !ok {
                    break;
                }
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // Everything queued has been written. A client that is still running gets a
                // moment to read it before it is hung up.
                if process.try_exit_code().is_none() {
                    std::thread::sleep(CLOSE_SETTLE);
                }
                break;
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                // The terminal ends when the tmux client does, not only when the PTY reports
                // end of file: a stray process holding the PTY open must not keep it alive.
                // Give the reader a moment to deliver what the client said last.
                match exited_at {
                    None if process.try_exit_code().is_some() => {
                        exited_at = Some(std::time::Instant::now());
                    }
                    Some(at) if at.elapsed() > EXIT_DRAIN => {
                        output.lock().done = true;
                        output.wake.notify_one();
                        exited_at = None;
                        // Keep serving commands until the async side hangs up.
                        std::thread::sleep(WRITER_POLL);
                        continue;
                    }
                    _ => {}
                }
                std::thread::sleep(WRITER_POLL);
            }
        }
    }
    let _ = done.send(process.hang_up(REAP_GRACE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_ends_when_the_client_exits_even_if_something_else_holds_the_pty_open() {
        // The shell exits at once; the background sleep inherits the PTY and keeps it open, so
        // the output never reaches end of file.
        let opened = flight_node::TerminalProcess::spawn(
            "sh",
            &["-c".to_owned(), "sleep 3 & exit 3".to_owned()],
            &[("PATH".to_owned(), std::env::var("PATH").unwrap_or_default())],
            80,
            24,
        )
        .expect("spawn");
        let output = Arc::new(Output::default());
        let (commands, receiver) = mpsc::channel::<Command>(4);
        let (done, code) = oneshot::channel();
        let writer = {
            let output = output.clone();
            std::thread::spawn(move || write_loop(opened.process, receiver, &output, done))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !output.lock().done {
            assert!(
                std::time::Instant::now() < deadline,
                "the exit of the client was not noticed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(commands);
        let _ = writer.join();
        assert_eq!(code.blocking_recv().ok().flatten(), Some(3));
        drop(opened.reader);
    }
}
