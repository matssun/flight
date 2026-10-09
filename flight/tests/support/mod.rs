// SPDX-License-Identifier: MIT

//! Shared helpers for the end-to-end tests: private tmux servers, a fake `claude`, cleanup
//! guards. Nothing here touches the default tmux server.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const PERMIT_SCREEN: &str =
    include_str!("../../../flight-classify/tests/fixtures/claude-permit.txt");

pub fn tmux(sock: &Sock, args: &[&str]) -> Option<String> {
    let out = Command::new("tmux")
        .args(sock.args())
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A tmux server on a private socket, killed and unlinked on drop.
pub enum Sock {
    Named(String),
    Path(PathBuf),
}

impl Sock {
    fn args(&self) -> [String; 2] {
        match self {
            Self::Named(n) => ["-L".into(), n.clone()],
            Self::Path(p) => ["-S".into(), p.to_string_lossy().into_owned()],
        }
    }
}

impl Drop for Sock {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(self.args())
            .arg("kill-server")
            .output();
        if let Self::Path(p) = self {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Removes the test's scratch directory even if an assertion panics.
pub struct TempDir(pub PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Removes tmux's socket file for a named socket (`-L`), which `kill-server` can leave behind.
pub struct NamedSocketFile(pub String);

impl Drop for NamedSocketFile {
    fn drop(&mut self) {
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default();
        for base in [
            format!("/private/tmp/tmux-{uid}"),
            format!("/tmp/tmux-{uid}"),
        ] {
            let _ = std::fs::remove_file(Path::new(&base).join(&self.0));
        }
    }
}

pub fn fake_claude(dir: &Path) -> Option<PathBuf> {
    Command::new("tmux").arg("-V").output().ok()?;
    let src = dir.join("fake.c");
    std::fs::write(
        &src,
        "#include <unistd.h>\nint main(void){sleep(300);return 0;}\n",
    )
    .ok()?;
    let bin = dir.join("claude");
    let ok = Command::new("cc")
        .arg("-o")
        .arg(&bin)
        .arg(&src)
        .status()
        .ok()?
        .success();
    ok.then_some(bin)
}

pub fn wait_for(sock: &Sock, pane: &str, want: &[&str]) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let screen = tmux(sock, &["capture-pane", "-p", "-t", pane]).unwrap_or_default();
        if want.iter().all(|w| screen.contains(w)) || Instant::now() > deadline {
            return screen;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn flight() -> Command {
    Command::new(env!("CARGO_BIN_EXE_flight"))
}

/// A child process killed on drop.
pub struct Proc(pub Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn run_ok(base: &Path, args: &[&str]) -> String {
    let out = flight()
        .args(args)
        .arg("--config-dir")
        .arg(base)
        .output()
        .expect("run flight");
    assert!(
        out.status.success(),
        "flight {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn start_orchestrator(base: &Path) -> (Proc, String) {
    let mut child = flight()
        .args([
            "orchestrator",
            "run",
            "--listen",
            "127.0.0.1:0",
            "--name",
            "e2e-orch",
            "--config-dir",
        ])
        .arg(base)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start orchestrator");
    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut addr = None;
    for _ in 0..3 {
        let line = lines.next().expect("orchestrator output").expect("line");
        if let Some(a) = line.strip_prefix("listening on ") {
            addr = Some(a.to_owned());
        }
    }
    (Proc(child), addr.expect("listening line"))
}

/// A just-enough terminal emulator for a ratatui dashboard: it follows cursor moves and prints,
/// ignores styling, and so can be asked what is on the screen rather than what was written
/// (ratatui rewrites only the cells that changed, so the byte stream is not readable text).
pub struct Screen {
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
    pending: String,
}

impl Screen {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            cells: vec![vec![' '; cols]; rows],
            row: 0,
            col: 0,
            pending: String::new(),
        }
    }

    pub fn feed(&mut self, chunk: &str) {
        let data = std::mem::take(&mut self.pending) + chunk;
        let chars: Vec<char> = data.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\x1b' => {
                    // An escape sequence cut by the end of the chunk waits for the rest.
                    let Some(&next) = chars.get(i + 1) else {
                        self.pending = chars[i..].iter().collect();
                        return;
                    };
                    if next != '[' {
                        i += 2;
                        continue;
                    }
                    let Some(end) = chars[i + 2..].iter().position(|c| ('@'..='~').contains(c))
                    else {
                        self.pending = chars[i..].iter().collect();
                        return;
                    };
                    let params: String = chars[i + 2..i + 2 + end].iter().collect();
                    self.csi(&params, chars[i + 2 + end]);
                    i += 3 + end;
                }
                '\r' => {
                    self.col = 0;
                    i += 1;
                }
                '\n' => {
                    self.row += 1;
                    i += 1;
                }
                c => {
                    if let Some(cell) = self
                        .cells
                        .get_mut(self.row)
                        .and_then(|r| r.get_mut(self.col))
                    {
                        *cell = c;
                    }
                    self.col += 1;
                    i += 1;
                }
            }
        }
    }

    fn csi(&mut self, params: &str, op: char) {
        let nums: Vec<usize> = params
            .trim_start_matches('?')
            .split(';')
            .map(|p| p.parse().unwrap_or(0))
            .collect();
        let n =
            |i: usize, default: usize| nums.get(i).copied().filter(|v| *v > 0).unwrap_or(default);
        match op {
            'H' | 'f' => {
                self.row = n(0, 1) - 1;
                self.col = n(1, 1) - 1;
            }
            'C' => self.col += n(0, 1),
            'J' => {
                for row in &mut self.cells {
                    row.iter_mut().for_each(|c| *c = ' ');
                }
            }
            _ => {}
        }
    }

    /// The screen as lines, trailing spaces trimmed.
    pub fn text(&self) -> String {
        self.cells
            .iter()
            .map(|r| r.iter().collect::<String>().trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A TCP proxy to `target` that delays every chunk by `one_way` in each direction, so a round
/// trip costs `2 * one_way` on top of the loopback. Returns the address to dial. Test tooling
/// for measuring what latency does to connection setup; it never inspects the bytes (the
/// connection is mutually authenticated TLS end to end).
#[allow(dead_code)]
pub fn delay_proxy(target: &str, one_way: Duration) -> String {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    let listener = TcpListener::bind("127.0.0.1:0").expect("proxy bind");
    let address = listener.local_addr().expect("proxy addr").to_string();
    let target = target.to_owned();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let Ok(server) = TcpStream::connect(&target) else {
                continue;
            };
            let _ = (client.set_nodelay(true), server.set_nodelay(true));
            for (mut from, mut to) in [
                (
                    client.try_clone().expect("clone"),
                    server.try_clone().expect("clone"),
                ),
                (server, client),
            ] {
                let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();
                std::thread::spawn(move || {
                    let mut buf = vec![0u8; 16 * 1024];
                    while let Ok(n) = from.read(&mut buf) {
                        if n == 0
                            || tx
                                .send((Instant::now() + one_way, buf[..n].to_vec()))
                                .is_err()
                        {
                            break;
                        }
                    }
                });
                std::thread::spawn(move || {
                    while let Ok((due, bytes)) = rx.recv() {
                        let now = Instant::now();
                        if due > now {
                            std::thread::sleep(due - now);
                        }
                        if to.write_all(&bytes).is_err() {
                            break;
                        }
                    }
                    let _ = to.shutdown(std::net::Shutdown::Both);
                });
            }
        }
    });
    address
}

/// Replace the orchestrator address in an enrollment bundle with a delaying proxy when
/// `FLIGHT_MEASURE_RTT_MS` asks for a simulated round trip time.
#[allow(dead_code)]
pub fn through_proxy(bundle: &str, real: &str) -> String {
    let Some(rtt) = std::env::var("FLIGHT_MEASURE_RTT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
    else {
        return bundle.to_owned();
    };
    let proxy = delay_proxy(real, Duration::from_micros(rtt * 500));
    bundle
        .split_whitespace()
        .map(|w| {
            if w.starts_with("address=") {
                format!("address={proxy}")
            } else {
                w.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
