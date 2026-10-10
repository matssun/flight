// SPDX-License-Identifier: MIT

//! The two observers against a real tmux server on a private socket (skipped without tmux).
//! Agent panes are a copy of `cat` named `claude`: tmux reports the command as
//! `claude`, and whatever is typed into the pane is echoed onto its screen.

use flight_node::{
    ControlLink, ControlSkipObserver, PaneObservation, PaneObserver, Round, SequentialObserver,
    ServerOutcome, TmuxServers, Unavailable,
};
use flight_state::ServerId;
use flight_tmux::{ControlConnection, SystemRunner, Tmux, TmuxEndpoint};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Live {
    endpoint: TmuxEndpoint,
    tmux: Tmux,
    claude: PathBuf,
    dir: PathBuf,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let dir = std::env::temp_dir().join(format!("flight-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let claude = dir.join("claude");
        let cat = String::from_utf8(Command::new("which").arg("cat").output().ok()?.stdout).ok()?;
        let _ = std::fs::remove_file(&claude);
        // A copy, not a symlink: tmux reports the resolved program's name. On macOS a copied
        // system binary only runs once it is signed again.
        std::fs::copy(cat.trim(), &claude).ok()?;
        if cfg!(target_os = "macos") {
            Command::new("codesign")
                .args(["--force", "-s", "-"])
                .arg(&claude)
                .output()
                .ok()?;
        }
        let endpoint =
            TmuxEndpoint::named(&format!("flight-test-{}-{tag}", std::process::id())).ok()?;
        Some(Self {
            tmux: Tmux::new(endpoint.clone()),
            endpoint,
            claude,
            dir,
        })
    }

    fn agent(&self, session: &str) {
        let command = self.claude.to_string_lossy().into_owned();
        self.tmux
            .new_session_running(session, "/tmp", &command)
            .unwrap();
    }

    fn type_into(&self, session: &str, text: &str) {
        self.tmux
            .runner()
            .run(&["send-keys", "-t", &format!("={session}:"), text, "Enter"])
            .unwrap();
    }

    fn observers(&self) -> (SequentialObserver, ControlSkipObserver) {
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(self.endpoint.clone())),
        );
        let servers = Arc::new(servers);
        let mut control = ControlSkipObserver::new(servers.clone());
        let endpoint = self.endpoint.clone();
        control.watch(ServerId::new("live"), move || {
            Ok(Box::new(ControlConnection::open(&endpoint)?) as Box<dyn ControlLink>)
        });
        (SequentialObserver::new(servers), control)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

use flight_tmux::TmuxRunner;

fn panes(rounds: &[Round]) -> Vec<PaneObservation> {
    match &rounds[0].outcome {
        ServerOutcome::Observed(p) => p.clone(),
        other => panic!("not observed: {other:?}"),
    }
}

/// `focused` differs by design while only our own control client is attached; everything else
/// must be identical.
fn same_but_focus(a: &[Round], b: &[Round]) -> bool {
    let strip = |rounds: &[Round]| {
        let mut p = panes(rounds);
        p.iter_mut().for_each(|x| x.focused = false);
        p
    };
    strip(a) == strip(b)
}

fn summary(a: &[Round], b: &[Round]) -> String {
    let show = |rounds: &[Round]| {
        panes(rounds)
            .iter()
            .map(|p| {
                format!(
                    "{} pid {} cmd {} title {:?} lines {} {:?}",
                    p.pane.as_str(),
                    p.pid,
                    p.command,
                    p.title,
                    p.screen_lines.len(),
                    p.screen_lines.iter().rev().take(2).collect::<Vec<_>>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n  ")
    };
    format!("control:\n  {}\nreference:\n  {}", show(a), show(b))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(12);
    while Instant::now() < end {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("never happened: {what}");
}

#[test]
fn the_control_observer_agrees_with_the_reference_on_a_real_server() {
    let Some(live) = Live::start("agree") else {
        return;
    };
    live.agent("a");
    live.agent("b");
    let (mut reference, mut control) = live.observers();

    // New panes start as a shell; wait until both have become the agent.
    until("both panes are agents", || {
        panes(&reference.observe(now())).len() == 2
    });
    let (want, got) = (reference.observe(now()), control.observe(now()));
    assert_eq!(panes(&got).len(), 2, "{:?}", panes(&got));
    assert!(same_but_focus(&got, &want), "{}", summary(&got, &want));

    // Typed text appears on the screen; the control path sees it and stays equal.
    live.type_into("a", "hello from a");
    until("the change is seen", || {
        let (g, w) = (control.observe(now()), reference.observe(now()));
        panes(&g)
            .iter()
            .any(|p| p.screen_lines.iter().any(|l| l.contains("hello from a")))
            && same_but_focus(&g, &w)
    });

    // A new agent pane and a removed one.
    live.agent("c");
    live.tmux.kill_session("b").unwrap();
    until("membership follows", || {
        let (g, w) = (control.observe(now()), reference.observe(now()));
        panes(&g).len() == 2 && same_but_focus(&g, &w)
    });
}

#[test]
fn our_own_client_is_not_a_viewer_but_a_person_is() {
    let Some(live) = Live::start("focus") else {
        return;
    };
    live.agent("a");
    let (mut reference, mut control) = live.observers();

    // The reference path counts our control client as attached; the control path must not.
    let got = control.observe(now());
    let _ = reference.observe(now());
    assert!(
        !panes(&got)[0].focused,
        "our own client made the pane focused"
    );

    // Another client attached to the same session is a viewer for both paths.
    let _person = ControlConnection::open(&live.endpoint).unwrap();
    until("a viewer is seen", || {
        panes(&control.observe(now()))[0].focused
    });
    assert!(panes(&reference.observe(now()))[0].focused);
}

#[test]
fn a_server_restart_is_reported_then_followed_without_stale_screens() {
    let Some(live) = Live::start("restart") else {
        return;
    };
    live.agent("a");
    live.type_into("a", "first life");
    let (mut reference, mut control) = live.observers();
    until("first life seen", || {
        panes(&control.observe(now()))
            .iter()
            .any(|p| p.screen_lines.iter().any(|l| l.contains("first life")))
    });
    let _ = control.take_notes();

    live.tmux.kill_server().unwrap();
    // One reading of the clock for both: a round says when it was taken, and two readings can
    // fall either side of a second.
    let taken = now();
    let down = control.observe(taken);
    assert_eq!(
        down[0].outcome,
        ServerOutcome::Unavailable(Unavailable::NoServer)
    );
    assert_eq!(down, reference.observe(taken));

    // The new server reuses pane ids for different processes.
    live.agent("a");
    live.type_into("a", "second life");
    until("second life seen, first life gone", || {
        let g = control.observe(now());
        let p = panes(&g);
        let text: String = p
            .iter()
            .flat_map(|x| x.screen_lines.clone())
            .collect::<Vec<_>>()
            .join("\n");
        text.contains("second life") && !text.contains("first life")
    });
    until("recovery is reported", || {
        let _ = control.observe(now());
        control.take_notes().iter().any(|n| n.contains("restored"))
    });
    let (g, w) = (control.observe(now()), reference.observe(now()));
    assert!(same_but_focus(&g, &w));
}
