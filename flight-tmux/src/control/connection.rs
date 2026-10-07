// SPDX-License-Identifier: MIT

use super::protocol::{Event, Parser, Reply};
use crate::{SystemRunner, TmuxEndpoint, TmuxError, TmuxRunner};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Every command over one persistent `tmux -C` connection to one explicit endpoint,
/// pipelined: `run` sends all commands and returns one reply per command, in order.
///
/// Anything that makes the stream untrustworthy (connection closed, `%exit`, a `%end` that
/// does not match its `%begin`, no reply in time) is an error. The connection is then
/// unusable: drop it and `open` a new one.
///
/// Output notifications are switched off: tmux only sends `%output` for the panes of the
/// session the client is attached to, so they cannot say what changed across a whole server.
/// The client is an attached client of one session (it shows in `list-clients` and in that
/// session's `session_attached`; `client_pid` identifies it so a caller can discount it).
pub struct ControlConnection {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Result<Reply, String>>,
    session: String,
}

impl ControlConnection {
    /// Attach to the first session of the server at `endpoint`. Fails like a plain tmux call
    /// does when there is no server (`TmuxError::Failed` with tmux's message).
    pub fn open(endpoint: &TmuxEndpoint) -> Result<Self, TmuxError> {
        let sessions = SystemRunner::new(endpoint.clone())
            .run(&["list-sessions", "-F", "#{session_name}"])?
            .stdout;
        let session = sessions
            .lines()
            .next()
            .ok_or_else(|| TmuxError::Control("the server has no session to attach to".into()))?
            .to_owned();
        let mut child = Command::new("tmux")
            .args(crate::tmux_args(
                endpoint,
                &["-C", "attach-session", "-t", &format!("={session}")],
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(TmuxError::Spawn)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TmuxError::Control("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TmuxError::Control("no stdout".into()))?;
        let (tx, replies) = channel();
        std::thread::spawn(move || {
            let mut parser = Parser::default();
            let mut input = BufReader::new(stdout);
            let mut raw = Vec::new();
            loop {
                raw.clear();
                if !matches!(input.read_until(b'\n', &mut raw), Ok(n) if n > 0) {
                    return;
                }
                // Screens may carry bytes that are not UTF-8; one such line must not end the
                // reader (and with it the connection).
                let text = String::from_utf8_lossy(&raw);
                let line = text.strip_suffix('\n').unwrap_or(&text);
                let message = match parser.feed(line) {
                    Event::Nothing => continue,
                    Event::Reply(reply) => Ok(reply),
                    Event::Exit => Err("tmux ended the connection".to_owned()),
                    Event::Desync(why) => Err(format!("stream out of step: {why}")),
                };
                let stop = message.is_err();
                if tx.send(message).is_err() || stop {
                    return;
                }
            }
        });
        let mut connection = Self {
            child,
            stdin,
            replies,
            session,
        };
        // The first reply is the attach's own (empty) one.
        connection.expect(1)?;
        connection.run(&["refresh-client -f ignore-size,no-output".to_owned()])?;
        Ok(connection)
    }

    /// The process id of this connection's tmux client (what `#{client_pid}` reports).
    pub fn client_pid(&self) -> u32 {
        self.child.id()
    }

    /// The session this client attached to.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Send `commands` and return their replies, one per command, in order. A command that
    /// tmux rejected has `ok == false`; that is not a connection error.
    pub fn run(&mut self, commands: &[String]) -> Result<Vec<Reply>, TmuxError> {
        let mut batch = String::new();
        for c in commands {
            batch.push_str(c);
            batch.push('\n');
        }
        self.stdin
            .write_all(batch.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| TmuxError::Control(format!("write failed: {e}")))?;
        self.expect(commands.len())
    }

    fn expect(&mut self, n: usize) -> Result<Vec<Reply>, TmuxError> {
        (0..n)
            .map(|_| match self.replies.recv_timeout(REPLY_TIMEOUT) {
                Ok(Ok(reply)) => Ok(reply),
                Ok(Err(why)) => Err(TmuxError::Control(why)),
                Err(e) => Err(TmuxError::Control(format!("no reply: {e}"))),
            })
            .collect()
    }
}

impl Drop for ControlConnection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
