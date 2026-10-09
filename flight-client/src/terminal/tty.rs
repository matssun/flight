// SPDX-License-Identifier: MIT

use crate::presentation::{PresentationConfig, PresentationSession};
use crate::session::{Binding, SessionConfig, SessionOutcome, SessionStart, SurfaceSession};
use crate::terminal::{LocalTerminal, TerminalEnd};
use crate::{LinkHost, OrchestratedBackend, ShownSurface};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
use flight_proto::valid_term;
use rustix::event::{poll, PollFd, PollFlags};
use rustix::fd::OwnedFd;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Frames of output waiting for the user's terminal. One in hand plus this many.
const OUTPUT_QUEUE: usize = 2;
/// How often the terminal size is looked at.
const TICK: Duration = Duration::from_millis(100);

/// The size and `TERM` of the user's terminal, as an `OpenTerminal` wants them.
pub fn terminal_request_shape() -> (u16, u16, String) {
    let (cols, rows) = size().unwrap_or((80, 24));
    let term = std::env::var("TERM")
        .ok()
        .filter(|t| valid_term(t))
        .unwrap_or_else(|| "xterm-256color".to_owned());
    (cols.max(1), rows.max(1), term)
}

/// What the dashboard hands over when the user opens a surface.
pub struct SessionRequest {
    /// The terminal the dashboard asked the orchestrator for.
    pub id: Vec<u8>,
    pub shown: ShownSurface,
    pub binding: Binding,
    /// Keys the user typed after opening the surface and before the dashboard let go, in order.
    pub typed_ahead: Vec<u8>,
}

struct RawMode;

impl RawMode {
    fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

/// A pipe whose read end can be waited on together with the keyboard, so stopping the keyboard
/// reader is immediate instead of up to a polling period away.
struct Wake {
    read: OwnedFd,
    write: OwnedFd,
}

impl Wake {
    fn new() -> std::io::Result<Self> {
        let (read, write) = rustix::pipe::pipe()?;
        Ok(Self { read, write })
    }

    fn wake(&self) {
        let _ = rustix::io::write(&self.write, &[1]);
    }
}

/// The user's real terminal for the length of a session: raw mode, a thread reading the
/// keyboard, one watching the size, one writing output. Ending it restores the terminal.
struct Plumbing {
    _raw: RawMode,
    wake: Arc<Wake>,
    stop: Arc<AtomicBool>,
    stdin_thread: std::thread::JoinHandle<()>,
    size_thread: std::thread::JoinHandle<()>,
    writer_thread: std::thread::JoinHandle<()>,
}

impl Plumbing {
    fn start() -> Result<(Self, LocalTerminal), &'static str> {
        let raw = RawMode::enter().map_err(|_| "cannot put the terminal in raw mode")?;
        let wake = Arc::new(Wake::new().map_err(|_| "cannot create a wake-up pipe")?);
        let stop = Arc::new(AtomicBool::new(false));
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(8);
        let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>(4);
        let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(OUTPUT_QUEUE);
        let stdin_thread = {
            let (stop, wake) = (stop.clone(), wake.clone());
            std::thread::spawn(move || read_stdin(&stop, &wake.read, &input_tx))
        };
        let size_thread = {
            let stop = stop.clone();
            std::thread::spawn(move || watch_size(&stop, &resize_tx))
        };
        let writer_thread = std::thread::spawn(move || {
            let mut out = std::io::stdout();
            while let Some(chunk) = output_rx.blocking_recv() {
                if out.write_all(&chunk).and_then(|()| out.flush()).is_err() {
                    break;
                }
            }
        });
        let local = LocalTerminal {
            input: input_rx,
            resizes: resize_rx,
            output: output_tx,
        };
        let plumbing = Self {
            _raw: raw,
            wake,
            stop,
            stdin_thread,
            size_thread,
            writer_thread,
        };
        Ok((plumbing, local))
    }

    /// Stop the threads and put the terminal back. `reset` is written last; keys typed for
    /// something that is gone are discarded when `flush_keys`, so the dashboard does not read
    /// them as commands.
    fn finish(self, reset: &[u8], flush_keys: bool) {
        self.stop.store(true, Ordering::Relaxed);
        self.wake.wake();
        self.size_thread.thread().unpark();
        let _ = self.stdin_thread.join();
        let _ = self.size_thread.join();
        // The output sender went away with the session; the writer drains what it was given.
        let _ = self.writer_thread.join();
        let mut out = std::io::stdout();
        let _ = out.write_all(reset);
        let _ = out.flush();
        if flush_keys {
            let _ = rustix::termios::tcflush(
                rustix::stdio::stdin(),
                rustix::termios::QueueSelector::IFlush,
            );
        }
    }
}

/// Notices can carry words from a peer (a refusal's message): none of it may carry a control
/// character into the user's terminal.
fn say_on_stderr() -> Arc<dyn Fn(&str) + Send + Sync> {
    Arc::new(|text| {
        let plain: String = text.chars().filter(|c| !c.is_control()).collect();
        let _ = std::io::stderr().write_all(format!("\r\n{plain}\r\n").as_bytes());
    })
}

/// Show the surfaces of a workspace in the user's terminal until the session ends, then
/// restore the terminal. The link the dashboard already holds carries everything: nothing is
/// dialed to start, and switching surface does not leave this function.
pub fn run_session(link: &OrchestratedBackend, request: SessionRequest) -> SessionOutcome {
    let (plumbing, local) = match Plumbing::start() {
        Ok(started) => started,
        Err(why) => {
            return SessionOutcome {
                end: TerminalEnd::Lost(why.to_owned()),
                shown: None,
                undelivered: 0,
            }
        }
    };
    let host = Arc::new(LinkHost::new(link.clone(), request.shown.workspace.clone()));
    let session = SurfaceSession::new(host, SessionConfig::new(say_on_stderr()));
    let (cols, rows, _) = terminal_request_shape();
    let outcome = link.runtime().block_on(session.run(
        local,
        SessionStart {
            id: request.id,
            choice: request.shown.choice,
            binding: request.binding,
            typed_ahead: request.typed_ahead,
            size: (cols, rows),
        },
    ));
    plumbing.finish(
        &SessionConfig::new(Arc::new(|_| {})).reset,
        outcome.end != TerminalEnd::UserLeft && outcome.end != TerminalEnd::Presenting,
    );
    outcome
}

/// Show several surfaces of a workspace at once, arranged by `layout`, until the user leaves or
/// nothing can be shown, then restore the terminal. Returns how it ended, with the layout as
/// the user left it.
pub fn run_presentation(
    link: &OrchestratedBackend,
    workspace: flight_ui::WorkspaceKey,
    layout: flight_present::Layout,
) -> crate::presentation::PresentationOutcome {
    let (plumbing, local) = match Plumbing::start() {
        Ok(started) => started,
        Err(why) => {
            return crate::presentation::PresentationOutcome {
                end: TerminalEnd::Lost(why.to_owned()),
                layout,
                undelivered: 0,
            }
        }
    };
    let host = Arc::new(LinkHost::new(link.clone(), workspace));
    let session =
        PresentationSession::new(host, PresentationConfig::for_workspace(say_on_stderr()));
    let (cols, rows, _) = terminal_request_shape();
    let outcome = link
        .runtime()
        .block_on(session.run(local, layout, (cols, rows)));
    plumbing.finish(b"", outcome.end != TerminalEnd::UserLeft);
    outcome
}

/// Raw bytes from the terminal. Waits on the keyboard and the wake-up pipe together, so it ends
/// at once when stopped, and never sits in a `read` that would take the next keystroke away
/// from whatever uses the terminal after us.
fn read_stdin(stop: &AtomicBool, wake: &OwnedFd, input: &mpsc::Sender<Vec<u8>>) {
    let stdin = rustix::stdio::stdin();
    let mut buf = vec![0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let mut fds = [
            PollFd::new(&stdin, PollFlags::IN),
            PollFd::new(wake, PollFlags::IN),
        ];
        if poll(&mut fds, -1).is_err() {
            continue;
        }
        let woken = fds
            .get(1)
            .is_some_and(|fd| fd.revents().contains(PollFlags::IN));
        let ready = fds.first().is_some_and(|fd| !fd.revents().is_empty());
        if woken || stop.load(Ordering::Relaxed) {
            return;
        }
        if !ready {
            continue;
        }
        match std::io::stdin().lock().read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let Some(bytes) = buf.get(..n) else { return };
                if input.blocking_send(bytes.to_vec()).is_err() {
                    return;
                }
            }
        }
    }
}

fn watch_size(stop: &AtomicBool, resizes: &mpsc::Sender<(u16, u16)>) {
    let mut last = size().ok();
    while !stop.load(Ordering::Relaxed) {
        std::thread::park_timeout(TICK);
        let now = size().ok();
        if now != last {
            last = now;
            if let Some(dims) = now {
                if resizes.blocking_send(dims).is_err() {
                    return;
                }
            }
        }
    }
}
