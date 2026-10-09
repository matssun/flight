// SPDX-License-Identifier: MIT

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

/// A child running on a PTY the node owns. Writing, resizing and hanging up are `&mut self`
/// because one thread drives them; the output side is the separate [`OpenedTerminal::reader`].
pub struct TerminalProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

/// A started terminal: the process handle and the blocking output stream of its PTY.
pub struct OpenedTerminal {
    pub process: TerminalProcess,
    /// Blocks until output or end of file. Ends (with `Ok(0)` or an error) once the child
    /// has gone away.
    pub reader: Box<dyn Read + Send>,
    /// Ask tmux to redraw this client's whole screen. Used after output was discarded because
    /// the far end could not keep up. May block briefly; failures are ignored.
    pub redraw: Redraw,
    /// Remove whatever the terminal made besides its process (a view session), after the
    /// process is gone. Must be harmless to run when there is nothing left to remove.
    pub cleanup: Redraw,
}

/// How to make a terminal's tmux client repaint itself (also used for other small actions
/// taken on a terminal's behalf).
pub type Redraw = Box<dyn Fn() + Send + Sync>;

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl TerminalProcess {
    /// Start `program args` on a fresh PTY of `cols` x `rows` with exactly `env` as its
    /// environment (nothing is inherited).
    pub fn spawn(
        program: &str,
        args: &[String],
        env: &[(String, String)],
        cols: u16,
        rows: u16,
    ) -> io::Result<OpenedTerminal> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io_err)?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(io_err)?;
        // Our copy of the slave must close, or the output never reaches end of file.
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(io_err)?;
        let writer = pair.master.take_writer().map_err(io_err)?;
        Ok(OpenedTerminal {
            process: Self {
                master: pair.master,
                writer,
                child,
            },
            reader,
            redraw: Box::new(|| {}),
            cleanup: Box::new(|| {}),
        })
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io_err)
    }

    /// Hang the child up (SIGHUP, which a tmux client treats as a detach) and reap it,
    /// waiting at most `grace`. Returns its exit code if it was reaped in time.
    pub fn hang_up(&mut self, grace: Duration) -> Option<i32> {
        let _ = self.child.kill();
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(exit_code(&status)),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return None,
            }
        }
    }

    /// The exit code if the child has exited; never blocks.
    pub fn try_exit_code(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(exit_code(&status)),
            _ => None,
        }
    }

    /// Whether the program on the PTY has taken the terminal over (left line-at-a-time mode, as a
    /// tmux client does as it starts). Until then input written to the PTY waits in the kernel
    /// and is read by whatever starts; hanging the child up before it starts discards it.
    pub fn client_ready(&self) -> bool {
        self.master.get_termios().is_some_and(|t| {
            !t.local_flags
                .contains(nix::sys::termios::LocalFlags::ICANON)
        })
    }

    /// A handle that can hang the child up without owning the process, so another thread can
    /// end a terminal whose owner is blocked writing to it.
    pub fn hang_up_handle(&self) -> HangUp {
        HangUp(self.child.clone_killer())
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }
}

/// Hangs a terminal's tmux client up from any thread.
pub struct HangUp(Box<dyn portable_pty::ChildKiller + Send + Sync>);

impl HangUp {
    /// Send the hang-up signal. Harmless if the child is already gone.
    pub fn hang_up(&mut self) {
        let _ = self.0.kill();
    }
}

impl Clone for HangUp {
    fn clone(&self) -> Self {
        Self(self.0.clone_killer())
    }
}

fn exit_code(status: &portable_pty::ExitStatus) -> i32 {
    i32::try_from(status.exit_code()).unwrap_or(i32::MAX)
}

impl Drop for TerminalProcess {
    /// A terminal never outlives its handle: no PTY child is left behind.
    fn drop(&mut self) {
        self.hang_up(Duration::from_secs(2));
    }
}
