// SPDX-License-Identifier: MIT

//! End to end: the real `flight` binary running its interactive TUI inside a private tmux
//! server, driven by real keystrokes, watching another private tmux server that hosts fake
//! agents. Skipped when tmux or a C compiler (for the fake `claude` binary) is missing.
//! Never touches the default tmux server.

mod support;

use std::time::{Duration, Instant};
use support::*;

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

    let first = wait_for(&ui, "ui:0", &["NEEDS YOU", "nga", "api", "waiting"]);
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
            .find(|l| l.contains('▌') && l.contains("claude"))
            .map(str::to_owned)
    };
    assert!(
        cursor(&first).unwrap_or_default().contains("api"),
        "cursor starts on the first session that needs the user:\n{first}"
    );

    tmux(&ui, &["send-keys", "-t", "ui:0", "Down"]).unwrap();
    let moved = wait_for(&ui, "ui:0", &["nga  WAITING", "Do you want to proceed?"]);
    assert!(
        cursor(&moved).unwrap_or_default().contains("nga"),
        "Down moved the cursor to nga:\n{moved}"
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
