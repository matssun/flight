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
}

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

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
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
