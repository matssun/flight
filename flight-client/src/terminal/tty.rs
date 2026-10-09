// SPDX-License-Identifier: MIT

use crate::terminal::{relay, Lease, TerminalEnd};
use crate::ClientConfig;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
use flight_proto::valid_term;
use flight_transport::TerminalClient;
use rustix::event::{poll, PollFd, PollFlags};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Frames of output waiting for the user's terminal. One in hand plus this many.
const OUTPUT_QUEUE: usize = 2;
/// How often the terminal size and the stop flag are looked at.
const TICK: Duration = Duration::from_millis(100);
/// Put the terminal back in a plain state: leave the alternate screen, show the cursor,
/// reset attributes, switch mouse reporting and bracketed paste off. Whatever the remote
/// tmux left on when it went away.
const RESET: &[u8] =
    b"\x1b[?1049l\x1b[?25h\x1b[0m\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l";

/// The size and `TERM` of the user's terminal, as an `OpenTerminal` wants them.
pub fn terminal_request_shape() -> (u16, u16, String) {
    let (cols, rows) = size().unwrap_or((80, 24));
    let term = std::env::var("TERM")
        .ok()
        .filter(|t| valid_term(t))
        .unwrap_or_else(|| "xterm-256color".to_owned());
    (cols.max(1), rows.max(1), term)
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

/// Show the terminal `terminal_id` in the user's terminal until it ends, then restore the
/// terminal. `Ctrl-Space` then `q` leaves at any time.
pub fn run_terminal(config: &ClientConfig, terminal_id: &[u8]) -> TerminalEnd {
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        return TerminalEnd::Lost("cannot start the terminal runtime".to_owned());
    };
    let client = match runtime.block_on(TerminalClient::connect_ui(
        &config.address,
        &config.identity,
        &config.orchestrator,
        terminal_id,
    )) {
        Ok(client) => client,
        Err(e) => return TerminalEnd::Lost(e.to_string()),
    };
    let lease = match runtime.block_on(Lease::connect(config, terminal_id)) {
        Ok(lease) => lease,
        Err(why) => return TerminalEnd::Lost(why),
    };
    let Ok(_raw) = RawMode::enter() else {
        return TerminalEnd::Lost("cannot put the terminal in raw mode".to_owned());
    };
    let (sender, receiver) = client.split();
    let stop = Arc::new(AtomicBool::new(false));
    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(8);
    let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>(4);
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(OUTPUT_QUEUE);

    let stdin_thread = {
        let (stop, input_tx) = (stop.clone(), input_tx);
        std::thread::spawn(move || read_stdin(&stop, &input_tx))
    };
    let size_thread = {
        let (stop, resize_tx) = (stop.clone(), resize_tx);
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

    let hint = || {
        let _ = std::io::stderr().write_all(
            b"\r\n[Ctrl-Space q leaves; Ctrl-Space Ctrl-Space sends a literal Ctrl-Space]\r\n",
        );
    };
    let end = runtime.block_on(relay(
        sender, receiver, input_rx, resize_rx, output_tx, hint, lease,
    ));

    stop.store(true, Ordering::Relaxed);
    let _ = stdin_thread.join();
    let _ = size_thread.join();
    // The output sender went away with the relay; the writer drains what it was given.
    let _ = writer_thread.join();
    let mut out = std::io::stdout();
    let _ = out.write_all(RESET).and_then(|()| out.flush());
    end
}

/// Raw bytes from the terminal, without ever blocking past `stop`: a thread stuck in `read`
/// would take the next keystroke away from whatever uses the terminal after us.
fn read_stdin(stop: &AtomicBool, input: &mpsc::Sender<Vec<u8>>) {
    let stdin = rustix::stdio::stdin();
    let mut buf = vec![0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
        let ready = poll(&mut fds, 100);
        if !matches!(ready, Ok(n) if n > 0) {
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
        std::thread::sleep(TICK);
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
