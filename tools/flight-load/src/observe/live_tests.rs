// SPDX-License-Identifier: MIT

//! The observation strategies against a real tmux server on a private `-L flight-test-*`
//! socket (never the default server). Skipped when tmux is absent. Every scenario runs over
//! every transport and policy, because the point is that they all agree with reality.

use super::args::Policy;
use super::control::Control;
use super::observer::Observer;
use super::subprocess::Subprocess;
use super::transport::Transport;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Server {
    socket: String,
}

impl Server {
    fn start() -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        Some(Self {
            socket: format!(
                "flight-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
        })
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .args(["-u", "-L", &self.socket])
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn session(&self, name: &str, command: &str) {
        self.tmux(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            "80",
            "-y",
            "24",
            command,
        ]);
    }

    /// Type `echo MARK_<tag>` into the session's shell. The quotes keep the typed command
    /// line from containing the marker; only the command's output does.
    fn say(&self, name: &str, tag: &str) {
        self.tmux(&[
            "send-keys",
            "-t",
            name,
            &format!("echo MA''RK_{tag}"),
            "Enter",
        ]);
    }

    fn pane_of(&self, name: &str) -> String {
        self.tmux(&["list-panes", "-t", name, "-F", "#{pane_id}"])
            .trim()
            .to_owned()
    }

    fn kill_server(&self) {
        self.tmux(&["kill-server"]);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill_server();
    }
}

fn screen<T: Transport>(o: &mut Observer<T>, pane: &str) -> Option<String> {
    let round = o.round().ok()?;
    round
        .screens
        .into_iter()
        .find(|(id, _)| id == pane)
        .map(|(_, s)| s)
}

/// Rounds until `pane` shows `needle`; panics with the last screen otherwise.
fn wait_for<T: Transport>(o: &mut Observer<T>, pane: &str, needle: &str) {
    let end = Instant::now() + Duration::from_secs(8);
    let mut last = None;
    while Instant::now() < end {
        last = screen(o, pane);
        if last.as_deref().is_some_and(|s| s.contains(needle)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{pane} never showed {needle}; last screen: {last:?}");
}

fn panes<T: Transport>(o: &mut Observer<T>) -> Vec<String> {
    let mut ids: Vec<String> = o
        .round()
        .unwrap()
        .screens
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    ids
}

fn changes_creation_and_removal<T: Transport>(srv: &Server, mut o: Observer<T>) {
    srv.session("a", "sh");
    srv.session("b", "sh");
    let (a, b) = (srv.pane_of("a"), srv.pane_of("b"));
    let ids = panes(&mut o);
    assert!(ids.contains(&a) && ids.contains(&b), "{ids:?}");

    srv.say("a", "one");
    wait_for(&mut o, &a, "MARK_one");
    // Unchanged neighbour is still right; a change to it is seen too.
    srv.say("b", "two");
    wait_for(&mut o, &b, "MARK_two");

    srv.session("c", "sh");
    let c = srv.pane_of("c");
    srv.say("c", "three");
    wait_for(&mut o, &c, "MARK_three");

    srv.tmux(&["kill-session", "-t", "b"]);
    let ids = panes(&mut o);
    assert!(
        ids.contains(&a) && ids.contains(&c) && !ids.contains(&b),
        "{ids:?}"
    );
}

fn server_restart_never_shows_the_old_server<T: Transport>(srv: &Server, mut o: Observer<T>) {
    srv.session("a", "sh");
    let old = srv.pane_of("a");
    srv.say("a", "old");
    wait_for(&mut o, &old, "MARK_old");

    srv.kill_server();
    assert!(
        o.round().is_err(),
        "a round against a dead server must fail"
    );

    // A new server reuses pane id %0 for a different process.
    srv.session("a", "sh");
    srv.say("a", "new");
    let end = Instant::now() + Duration::from_secs(8);
    while o.recover().is_err() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let new = srv.pane_of("a");
    wait_for(&mut o, &new, "MARK_new");
    let shown = screen(&mut o, &new).unwrap();
    assert!(
        !shown.contains("MARK_old"),
        "stale screen from the old server: {shown}"
    );
}

fn high_output_does_not_stall_or_hide_a_quiet_pane<T: Transport>(srv: &Server, mut o: Observer<T>) {
    srv.session("noisy", "yes");
    srv.session("quiet", "sh");
    let quiet = srv.pane_of("quiet");
    for _ in 0..5 {
        let started = Instant::now();
        o.round().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    srv.say("quiet", "calm");
    wait_for(&mut o, &quiet, "MARK_calm");
    let noisy = srv.pane_of("noisy");
    assert!(screen(&mut o, &noisy).unwrap().contains('y'));
}

fn run_all<T: Transport>(make: impl Fn(&Server) -> Option<Observer<T>>) {
    type Scenario<T> = fn(&Server, Observer<T>);
    let scenarios: [(&str, Scenario<T>); 3] = [
        ("changes", changes_creation_and_removal),
        ("restart", server_restart_never_shows_the_old_server),
        ("output", high_output_does_not_stall_or_hide_a_quiet_pane),
    ];
    for (name, scenario) in scenarios {
        let Some(srv) = Server::start() else { return };
        // The control client attaches to an existing session, so one must exist first.
        srv.session("seed", "sh");
        let Some(observer) = make(&srv) else {
            panic!("{name}: could not build the observer");
        };
        scenario(&srv, observer);
    }
}

#[test]
fn subprocess_all() {
    run_all(|s| Some(Observer::new(Subprocess::new(&s.socket, 4), Policy::All)));
}

#[test]
fn subprocess_skip() {
    run_all(|s| {
        Some(Observer::new(
            Subprocess::new(&s.socket, 1),
            Policy::SkipUnchanged,
        ))
    });
}

#[test]
fn control_all() {
    run_all(|s| {
        Control::start(&s.socket)
            .ok()
            .map(|c| Observer::new(c, Policy::All))
    });
}

#[test]
fn control_skip() {
    run_all(|s| {
        Control::start(&s.socket)
            .ok()
            .map(|c| Observer::new(c, Policy::SkipUnchanged))
    });
}

/// Killing the control client mid-flight is an error, not stale data; after `recover` the
/// change made while it was down is seen.
#[test]
fn control_connection_loss_is_an_error_then_a_full_refresh() {
    let Some(srv) = Server::start() else { return };
    srv.session("a", "sh");
    let a = srv.pane_of("a");
    let control = Control::start(&srv.socket).unwrap();
    let client = control.client_pid();
    let mut o = Observer::new(control, Policy::SkipUnchanged);
    o.round().unwrap();

    Command::new("kill")
        .args(["-9", &client.to_string()])
        .status()
        .unwrap();
    srv.say("a", "down");
    assert!(o.round().is_err());

    o.recover().unwrap();
    wait_for(&mut o, &a, "MARK_down");
}
