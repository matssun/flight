// SPDX-License-Identifier: MIT

use super::transport::{capture_args, parse_list, PaneRow, Transport, LIST_FORMAT};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

/// Every command over one persistent `tmux -C` connection, pipelined. A reader thread splits
/// the stream into command responses (`%begin`..`%end`) and notifications; `%output` marks a
/// pane dirty. The control client is an attached client of one session (it shows in
/// `list-clients` and raises that session's `session_attached`): a real cost of this approach.
pub struct Control {
    child: Child,
    stdin: ChildStdin,
    blocks: Receiver<Vec<String>>,
    dirty: Arc<Mutex<HashSet<String>>>,
}

impl Control {
    /// Attach to `session`. With `events` false, `%output` notifications are switched off.
    pub fn start(socket: &str, session: &str, events: bool) -> Result<Self, String> {
        let mut child = Command::new("tmux")
            .args(["-u", "-L", socket, "-C", "attach-session", "-t", session])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, blocks) = channel();
        let dirty: Arc<Mutex<HashSet<String>>> = Arc::default();
        let seen = dirty.clone();
        std::thread::spawn(move || {
            let mut block: Option<Vec<String>> = None;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match block.as_mut() {
                    Some(lines) if line.starts_with("%end ") || line.starts_with("%error ") => {
                        let done = std::mem::take(lines);
                        block = None;
                        if tx.send(done).is_err() {
                            return;
                        }
                    }
                    Some(lines) => lines.push(line),
                    None if line.starts_with("%begin ") => block = Some(Vec::new()),
                    None => {
                        if let Some(rest) = line.strip_prefix("%output ") {
                            if let (Some(id), Ok(mut set)) = (rest.split(' ').next(), seen.lock()) {
                                set.insert(id.to_owned());
                            }
                        }
                    }
                }
            }
        });
        let mut control = Self {
            child,
            stdin,
            blocks,
            dirty,
        };
        // The first block is the attach's own (empty) response.
        let _ = control.blocks.recv();
        let flags = if events {
            "ignore-size"
        } else {
            "ignore-size,no-output"
        };
        control.send(&[format!("refresh-client -f {flags}")])?;
        control.expect(1)?;
        Ok(control)
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

    fn expect(&mut self, n: usize) -> Result<Vec<Vec<String>>, String> {
        (0..n)
            .map(|_| {
                self.blocks
                    .recv_timeout(std::time::Duration::from_secs(20))
                    .map_err(|e| e.to_string())
            })
            .collect()
    }
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
        let blocks = self.expect(1)?;
        Ok(parse_list(&blocks.concat().join("\n")))
    }

    fn capture(&mut self, ids: &[String]) -> Result<Vec<String>, String> {
        let commands: Vec<String> = ids.iter().map(|id| capture_args(id).join(" ")).collect();
        self.send(&commands)?;
        Ok(self
            .expect(ids.len())?
            .into_iter()
            .map(|lines| lines.join("\n"))
            .collect())
    }

    fn take_dirty(&mut self) -> Option<HashSet<String>> {
        self.dirty
            .lock()
            .ok()
            .map(|mut set| std::mem::take(&mut *set))
    }
}
