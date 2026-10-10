// SPDX-License-Identifier: MIT

//! `on_real_terminal` against a real pseudo-terminal. The terminal is process-wide state (raw
//! mode, the keyboard queue), so each case runs in a child process: this test binary run again,
//! with `FLIGHT_TTY_CHILD` naming the case, as the only process on a fresh pty. The child
//! reports what it saw as `KEY=value` lines; the parent reads them from the pty.

use super::reset;
use super::tty::{on_real_terminal, Ending};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rustix::event::{poll, PollFd, PollFlags};
use std::io::Read;
use std::time::{Duration, Instant};

const CASE: &str = "FLIGHT_TTY_CHILD";
const RAN: &str = "ran-on-the-terminal";

/// Run the child case `case` on a pty. Everything the terminal showed, until the child ended.
fn on_a_pty(case: &str) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("a pty");
    let mut cmd = CommandBuilder::new(std::env::current_exe().expect("this binary"));
    cmd.args([
        "terminal::tty_tests::child",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]);
    cmd.env(CASE, case);
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).expect("the child");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("a reader");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut shown = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => shown.extend(chunk),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if child.try_wait().expect("child state").is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = child.kill();
    String::from_utf8_lossy(&shown).into_owned()
}

/// The value the child reported for `key`.
fn said(shown: &str, key: &str) -> Option<String> {
    let at = shown.find(&format!("{key}="))?;
    let rest = shown.get(at + key.len() + 1..)?;
    Some(rest.chars().take_while(|c| !c.is_control()).collect())
}

/// What a failure message may show of `shown`: the typed burst and its echo left out.
fn brief(shown: &str) -> String {
    shown
        .chars()
        .filter(|c| *c != 'x' && *c != '\u{7}')
        .collect()
}

/// What the child does. Without `FLIGHT_TTY_CHILD` this is not a test of anything.
#[test]
fn child() {
    if std::env::var(CASE).is_err() {
        return;
    }
    let started = Instant::now();
    let inside = on_real_terminal(
        |why| panic!("could not take the terminal: {why}"),
        |local, size| {
            let raw = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
            let _ = local.output.blocking_send(RAN.as_bytes().to_vec());
            // Nothing is typed; the keyboard thread is waiting in poll.
            std::thread::sleep(Duration::from_millis(200));
            drop(local);
            (raw, size)
        },
        |_| Ending {
            reset: reset::FULL_SCREEN,
            flush_keys: false,
        },
    );
    let stopped_in = started.elapsed();
    let raw_after = crossterm::terminal::is_raw_mode_enabled().unwrap_or(true);
    println!("RAW_INSIDE={}", inside.0);
    println!("SIZE={}x{}", inside.1 .0, inside.1 .1);
    println!("RAW_AFTER={raw_after}");
    println!("MS={}", stopped_in.as_millis());
}

#[test]
fn the_terminal_is_raw_while_it_is_ours_and_restored_with_the_reset_written_after_all_output() {
    let shown = on_a_pty("idle");
    assert_eq!(
        said(&shown, "RAW_INSIDE").as_deref(),
        Some("true"),
        "{:?}",
        brief(&shown)
    );
    assert_eq!(
        said(&shown, "RAW_AFTER").as_deref(),
        Some("false"),
        "{:?}",
        brief(&shown)
    );
    assert_eq!(
        said(&shown, "SIZE").as_deref(),
        Some("80x24"),
        "{:?}",
        brief(&shown)
    );
    let reset_at = shown
        .find(std::str::from_utf8(reset::FULL_SCREEN).expect("ascii"))
        .unwrap_or_else(|| panic!("the reset was not written: {:?}", brief(&shown)));
    let ran_at = shown
        .find(RAN)
        .expect("what the session wrote reached the terminal");
    assert!(
        ran_at < reset_at,
        "output, then the reset: {:?}",
        brief(&shown)
    );
    assert!(
        shown
            .rfind("RAW_AFTER")
            .is_some_and(|after| reset_at < after),
        "the reset comes before the terminal is used again: {:?}",
        brief(&shown)
    );
}

#[test]
fn stopping_does_not_wait_for_a_key() {
    let shown = on_a_pty("idle");
    let ms: u128 = said(&shown, "MS").and_then(|m| m.parse().ok()).expect("MS");
    // The session runs 200 ms; the keyboard thread is blocked waiting for a key and must not
    // add its polling period or more.
    assert!(ms < 1500, "took {ms} ms: {:?}", brief(&shown));
}

#[test]
fn keys_typed_for_something_that_is_gone_are_discarded() {
    use rustix::fs::{open, Mode, OFlags};
    use rustix::pty::{grantpt, openpt, ptsname, unlockpt, OpenptFlags};
    use rustix::termios::{tcgetattr, tcsetattr, OptionalActions};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).expect("a pty master");
    grantpt(&master).expect("grant");
    unlockpt(&master).expect("unlock");
    let name = ptsname(&master, Vec::new()).expect("the slave's name");
    let slave = open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY,
        Mode::empty(),
    )
    .expect("slave");
    // Raw, so what was typed is readable without a newline, as it is under a session.
    let mut raw = tcgetattr(&slave).expect("attributes");
    raw.make_raw();
    tcsetattr(&slave, OptionalActions::Now, &raw).expect("raw");
    let waiting = |fd: &rustix::fd::OwnedFd| {
        let mut fds = [PollFd::new(fd, PollFlags::IN)];
        poll(&mut fds, 200).map(|n| n > 0).unwrap_or(false)
    };

    rustix::io::write(&master, b"typed for something that is gone").expect("typing");
    assert!(waiting(&slave), "the keys are there to be read");
    super::tty::discard_unread_keys(&slave);
    assert!(!waiting(&slave), "and are gone once discarded");

    // Keys typed after are not touched: the discard is of what was waiting.
    rustix::io::write(&master, b"next").expect("typing");
    assert!(waiting(&slave));
}
