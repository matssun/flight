// SPDX-License-Identifier: MIT

//! Shared helpers for the end-to-end tests: private tmux servers, a fake `claude`, cleanup
//! guards. Nothing here touches the default tmux server.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
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
