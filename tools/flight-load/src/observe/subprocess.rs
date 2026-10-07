// SPDX-License-Identifier: MIT

use super::transport::{capture_args, parse_list, PaneRow, Transport, LIST_FORMAT};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// One `tmux` subprocess per command, `workers` at a time (1 = what the node does today).
pub struct Subprocess {
    socket: String,
    workers: usize,
}

impl Subprocess {
    pub fn new(socket: &str, workers: usize) -> Self {
        Self {
            socket: socket.to_owned(),
            workers: workers.max(1),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("tmux")
            .args(["-u", "-L", &self.socket])
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    }
}

impl Transport for Subprocess {
    fn list(&mut self) -> Result<Vec<PaneRow>, String> {
        Ok(parse_list(&self.run(&[
            "list-panes",
            "-a",
            "-F",
            LIST_FORMAT,
        ])?))
    }

    fn capture(&mut self, ids: &[String]) -> Result<Vec<String>, String> {
        let next = AtomicUsize::new(0);
        let screens: Mutex<Vec<Option<String>>> = Mutex::new(vec![None; ids.len()]);
        std::thread::scope(|scope| {
            for _ in 0..self.workers.min(ids.len().max(1)) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(id) = ids.get(i) else { break };
                    let text = self.run(&capture_args(id)).unwrap_or_default();
                    if let Ok(mut slots) = screens.lock() {
                        if let Some(slot) = slots.get_mut(i) {
                            *slot = Some(text);
                        }
                    }
                });
            }
        });
        let slots = screens.into_inner().map_err(|e| e.to_string())?;
        Ok(slots.into_iter().map(Option::unwrap_or_default).collect())
    }
}
