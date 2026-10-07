// SPDX-License-Identifier: MIT

use super::protocol::{Event, Parser, Reply};
use super::transport::{capture_args, parse_list, PaneRow, Transport, LIST_FORMAT};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// Every command over one persistent `tmux -C` connection, pipelined. A reader thread splits
/// the stream into replies; anything that makes the stream untrustworthy (connection closed,
/// `%exit`, mismatched `%begin`/`%end`, no reply in time) is an error, and `recover` builds a
/// fresh connection. Output notifications are switched off: tmux only sends `%output` for
/// panes of the attached session, so they cannot tell a fleet-wide observer what changed.
/// The control client is an attached client of one session (it shows in `list-clients` and
/// raises that session's `session_attached`): a real cost of this approach.
pub struct Control {
    socket: String,
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Result<Reply, String>>,
}

impl Control {
    /// Attach to the first session of the server on `socket`.
    pub fn start(socket: &str) -> Result<Self, String> {
        let session = first_session(socket)?;
        let mut child = Command::new("tmux")
            .args(["-u", "-L", socket, "-C", "attach-session", "-t", &session])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, replies) = channel();
        std::thread::spawn(move || {
            let mut parser = Parser::default();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let message = match parser.feed(&line) {
                    Event::Nothing => continue,
                    Event::Reply(reply) => Ok(reply),
                    Event::Exit => Err("tmux ended the control connection".to_owned()),
                    Event::Desync(why) => Err(format!("control stream out of step: {why}")),
                };
                let stop = message.is_err();
                if tx.send(message).is_err() || stop {
                    return;
                }
            }
        });
        let mut control = Self {
            socket: socket.to_owned(),
            child,
            stdin,
            replies,
        };
        // The first reply is the attach's own (empty) one.
        control.expect(1)?;
        control.send(&["refresh-client -f ignore-size,no-output".to_owned()])?;
        control.expect(1)?;
        Ok(control)
    }

    #[cfg(test)]
    pub fn client_pid(&self) -> u32 {
        self.child.id()
    }

    fn send(&mut self, commands: &[String]) -> Result<(), String> {
        let mut batch = String::new();
        for c in commands {
            batch.push_str(c);
            batch.push('\n');
        }
        self.stdin
            .write_all(batch.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| e.to_string())
    }

    fn expect(&mut self, n: usize) -> Result<Vec<Reply>, String> {
        (0..n)
            .map(|_| match self.replies.recv_timeout(REPLY_TIMEOUT) {
                Ok(reply) => reply,
                Err(e) => Err(format!("no control reply: {e}")),
            })
            .collect()
    }
}

fn first_session(socket: &str) -> Result<String, String> {
    let out = Command::new("tmux")
        .args(["-u", "-L", socket, "list-sessions", "-F", "#{session_name}"])
        .output()
        .map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "no tmux session to attach to".to_owned())
}

impl Drop for Control {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Transport for Control {
    fn list(&mut self) -> Result<Vec<PaneRow>, String> {
        self.send(&[format!("list-panes -a -F '{LIST_FORMAT}'")])?;
        let reply = self.expect(1)?.into_iter().next().ok_or("no reply")?;
        if !reply.ok {
            return Err(format!("list-panes failed: {}", reply.lines.join(" ")));
        }
        parse_list(&reply.lines.join("\n"))
    }

    fn capture(&mut self, ids: &[String]) -> Result<Vec<Option<String>>, String> {
        let commands: Vec<String> = ids.iter().map(|id| capture_args(id).join(" ")).collect();
        self.send(&commands)?;
        Ok(self
            .expect(ids.len())?
            .into_iter()
            .map(|r| r.ok.then(|| r.lines.join("\n")))
            .collect())
    }

    fn recover(&mut self) -> Result<(), String> {
        *self = Self::start(&self.socket)?;
        Ok(())
    }
}
