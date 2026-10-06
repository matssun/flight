// SPDX-License-Identifier: MIT

//! End to end: the real `flight` binary running its interactive TUI inside a private tmux
//! server, driven by real keystrokes, watching another private tmux server that hosts fake
//! agents. Skipped when tmux or a C compiler (for the fake `claude` binary) is missing.
//! Never touches the default tmux server.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PERMIT_SCREEN: &str = include_str!("../../flight-classify/tests/fixtures/claude-permit.txt");

fn tmux(sock: &Sock, args: &[&str]) -> Option<String> {
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
enum Sock {
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
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Removes tmux's socket file for a named socket (`-L`), which `kill-server` can leave behind.
struct NamedSocketFile(String);

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

fn fake_claude(dir: &Path) -> Option<PathBuf> {
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

fn wait_for(sock: &Sock, pane: &str, want: &[&str]) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let screen = tmux(sock, &["capture-pane", "-p", "-t", pane]).unwrap_or_default();
        if want.iter().all(|w| screen.contains(w)) || Instant::now() > deadline {
            return screen;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn keys_move_the_cursor_between_panes_and_q_quits() {
    let dir = std::env::temp_dir().join(format!("flight-e2e-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let _cleanup = TempDir(dir.clone());
    let Some(claude) = fake_claude(&dir) else {
        return;
    };
    std::fs::write(dir.join("screen.txt"), PERMIT_SCREEN).unwrap();

    // The agents' tmux server (named socket, as `flight --socket` expects).
    let name = format!("flight-e2e-agents-{}", std::process::id());
    let _file = NamedSocketFile(name.clone());
    let agents = Sock::Named(name.clone());
    let run = format!(
        "cat {}; exec {}",
        dir.join("screen.txt").display(),
        claude.display()
    );
    for session in ["nga", "api"] {
        tmux(
            &agents,
            &["new-session", "-d", "-s", session, "-c", "/tmp", &run],
        )
        .expect("start agent session");
    }
    tmux(
        &agents,
        &["new-session", "-d", "-s", "scratch", "-c", "/tmp"],
    )
    .expect("start shell session");

    // A separate tmux server hosts the dashboard, so keystrokes reach a real terminal.
    let ui = Sock::Path(dir.join("ui.sock"));
    let cmd = format!(
        "{} --socket {name} --refresh 1",
        env!("CARGO_BIN_EXE_flight")
    );
    tmux(
        &ui,
        &[
            "new-session",
            "-d",
            "-s",
            "ui",
            "-x",
            "120",
            "-y",
            "30",
            &cmd,
        ],
    )
    .expect("start dashboard");

    let first = wait_for(&ui, "ui:0", &["ATTENTION", "nga", "api", "waiting"]);
    assert!(
        first.contains("nga") && first.contains("api"),
        "both agents listed:\n{first}"
    );
    assert!(
        !first.contains("scratch"),
        "the plain shell is hidden:\n{first}"
    );
    let cursor = |s: &str| {
        s.lines()
            .find(|l| l.contains("> ") && l.contains("Claude"))
            .map(str::to_owned)
    };
    assert!(
        cursor(&first).unwrap_or_default().contains("nga"),
        "cursor starts on the first attention pane:\n{first}"
    );

    tmux(&ui, &["send-keys", "-t", "ui:0", "Down"]).unwrap();
    let moved = wait_for(&ui, "ui:0", &["local / api", "Do you want to proceed?"]);
    assert!(
        cursor(&moved).unwrap_or_default().contains("api"),
        "Down moved the cursor to api:\n{moved}"
    );
    assert!(
        moved.contains("Do you want to proceed?"),
        "preview shows the pane's screen:\n{moved}"
    );

    tmux(&ui, &["send-keys", "-t", "ui:0", "q"]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while tmux(&ui, &["has-session", "-t", "ui"]).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        tmux(&ui, &["has-session", "-t", "ui"]).is_none(),
        "q quit the dashboard"
    );
}
