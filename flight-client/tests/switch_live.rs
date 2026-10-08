// SPDX-License-Identifier: MIT

//! Client identification and the local switch against a real tmux server on a private socket,
//! with terminals attached through a pty (skipped without tmux and python3).

use flight_client::{
    detect_placement, Presented, SshDestinations, SwitchError, SwitchTarget, Switcher, TmuxEnv,
    UiPlacement,
};
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use flight_tmux::{Tmux, TmuxEndpoint, TmuxRunner};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PTY_CLIENT: &str = r#"
import os, pty, sys, time, fcntl, termios, struct
pid, fd = pty.fork()
if pid == 0:
    os.execvp("tmux", ["tmux", "-L", sys.argv[1], "attach-session", "-t", sys.argv[2]])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
while True:
    try:
        os.read(fd, 65536)
    except OSError:
        time.sleep(0.1)
"#;

struct Live {
    name: String,
    tmux: Tmux,
    clients: Vec<Child>,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        Command::new("python3").arg("-V").output().ok()?;
        let name = format!("flight-test-{}-{tag}", std::process::id());
        let tmux = Tmux::new(TmuxEndpoint::named(&name).ok()?);
        Some(Self {
            name,
            tmux,
            clients: Vec::new(),
        })
    }

    fn raw(&self, args: &[&str]) -> String {
        self.tmux.runner().run(args).unwrap().stdout
    }

    fn session(&self, name: &str) {
        self.raw(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            "120",
            "-y",
            "40",
            "cat",
        ]);
    }

    /// Attach a terminal to `session` and wait until tmux lists it.
    fn attach(&mut self, session: &str) {
        let before = self.client_count();
        self.clients.push(
            Command::new("python3")
                .args(["-c", PTY_CLIENT, &self.name, session])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.client_count() == before {
            assert!(Instant::now() < deadline, "client never attached");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn client_count(&self) -> usize {
        self.tmux
            .runner()
            .run(&["list-clients", "-F", "#{client_name}"])
            .map_or(0, |o| o.stdout.lines().count())
    }

    fn env_of(&self, pane: &str, with_pane: bool) -> TmuxEnv {
        let socket = self.raw(&["display-message", "-p", "#{socket_path}"]);
        let info = self.raw(&["display-message", "-p", "-t", pane, "#{pid},#{session_id}"]);
        let (pid, sid) = info.trim().split_once(",").unwrap();
        TmuxEnv {
            tmux: Some(format!(
                "{},{pid},{}",
                socket.trim(),
                sid.trim_start_matches('$')
            )),
            tmux_pane: with_pane.then(|| pane.to_owned()),
        }
    }

    fn pane_of(&self, session: &str) -> (String, u32) {
        let out = self.raw(&[
            "list-panes",
            "-t",
            &format!("={session}"),
            "-F",
            "#{pane_id} #{pane_pid}",
        ]);
        let (id, pid) = out.trim().split_once(' ').unwrap();
        (id.to_owned(), pid.parse().unwrap())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        for c in &mut self.clients {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = self.tmux.kill_server();
    }
}

fn clients_of(p: UiPlacement) -> (Option<String>, Vec<String>) {
    match p {
        UiPlacement::InsideTmux { server, clients } => (server, clients),
        UiPlacement::OutsideTmux => panic!("outside"),
    }
}

#[test]
fn one_client_is_found_from_an_ordinary_pane_and_from_a_popup() {
    let Some(mut live) = Live::start("one") else {
        return;
    };
    live.session("ui");
    live.attach("ui");
    let (pane, _) = live.pane_of("ui");
    let (server, clients) = clients_of(detect_placement(&live.env_of(&pane, true)));
    assert_eq!(server.as_deref(), Some(live.name.as_str()));
    assert_eq!(clients.len(), 1, "{clients:?}");
    // Inside a popup `$TMUX_PANE` is empty; the session still comes from `$TMUX`.
    let (_, popup) = clients_of(detect_placement(&live.env_of(&pane, false)));
    assert_eq!(popup, clients);
}

#[test]
fn a_real_popup_has_no_pane_variable_and_still_resolves() {
    let Some(mut live) = Live::start("popup") else {
        return;
    };
    live.session("ui");
    live.attach("ui");
    let client = live.raw(&["list-clients", "-F", "#{client_name}"]);
    let out = std::env::temp_dir().join(format!("flight-popup-{}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    live.raw(&[
        "display-popup",
        "-c",
        client.trim(),
        "-E",
        &format!(
            "printf '%s|%s' \"$TMUX\" \"$TMUX_PANE\" > {}",
            out.display()
        ),
    ]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let seen = loop {
        if let Ok(text) = std::fs::read_to_string(&out) {
            break text;
        }
        assert!(Instant::now() < deadline, "popup never ran");
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = std::fs::remove_file(&out);
    let (tmux_var, pane_var) = seen.split_once('|').unwrap();
    assert!(!tmux_var.is_empty());
    assert_eq!(pane_var, "", "a popup has no $TMUX_PANE");
    let env = TmuxEnv {
        tmux: Some(tmux_var.to_owned()),
        tmux_pane: None,
    };
    let (server, clients) = clients_of(detect_placement(&env));
    assert_eq!(server.as_deref(), Some(live.name.as_str()));
    assert_eq!(clients.len(), 1, "{clients:?}");
}

#[test]
fn two_terminals_on_one_session_are_two_clients_so_nothing_is_guessed() {
    let Some(mut live) = Live::start("two") else {
        return;
    };
    live.session("ui");
    live.attach("ui");
    live.attach("ui");
    let (pane, _) = live.pane_of("ui");
    for with_pane in [true, false] {
        let (_, clients) = clients_of(detect_placement(&live.env_of(&pane, with_pane)));
        assert_eq!(clients.len(), 2, "{clients:?}");
    }
}

#[test]
fn a_control_client_is_not_a_terminal() {
    let Some(mut live) = Live::start("control") else {
        return;
    };
    live.session("ui");
    live.attach("ui");
    // Flight's own observer is a control client on the same server.
    let _watcher =
        flight_tmux::ControlConnection::open(&TmuxEndpoint::named(&live.name).unwrap()).unwrap();
    assert_eq!(live.client_count(), 2);
    let (pane, _) = live.pane_of("ui");
    let (_, clients) = clients_of(detect_placement(&live.env_of(&pane, true)));
    assert_eq!(clients.len(), 1, "{clients:?}");
}

fn target(live: &Live, pane: &str, pid: u32, session: &str) -> SwitchTarget {
    SwitchTarget {
        pane: PaneRef {
            host: HostId::new("this-node"),
            server: ServerId::new(live.name.clone()),
            pane: PaneId::new(pane),
        },
        pid,
        session: session.to_owned(),
    }
}

fn this_machine() -> Switcher {
    Switcher::new(Some(HostId::new("this-node")), SshDestinations::default())
}

fn never_remote(_: &PaneRef, _: u32) -> Result<(), String> {
    panic!("a local pane must not go through the orchestrator");
}

fn client_session(live: &Live) -> String {
    live.raw(&["list-clients", "-F", "#{client_session}"])
        .trim()
        .to_owned()
}

#[test]
fn enter_moves_the_one_terminal_to_the_pane_it_still_runs() {
    let Some(mut live) = Live::start("move") else {
        return;
    };
    live.session("ui");
    live.session("work");
    live.raw(&["split-window", "-d", "-t", "=work:", "cat"]);
    live.attach("ui");
    let (ui_pane, _) = live.pane_of("ui");
    let panes = live.raw(&[
        "list-panes",
        "-t",
        "=work:",
        "-F",
        "#{pane_id} #{pane_pid} #{pane_active}",
    ]);
    let inactive = panes.lines().find(|l| l.ends_with(" 0")).unwrap();
    let mut it = inactive.split(' ');
    let (pane, pid) = (it.next().unwrap(), it.next().unwrap().parse().unwrap());
    assert_eq!(client_session(&live), "ui");

    let env = live.env_of(&ui_pane, true);
    let outcome = this_machine().switch(&target(&live, pane, pid, "work"), &env, &mut never_remote);
    assert_eq!(outcome, Ok(Presented::ClientMoved));
    assert_eq!(client_session(&live), "work");
    let active = live.raw(&["display-message", "-p", "-t", "=work:", "#{pane_id}"]);
    assert_eq!(active.trim(), pane, "the selected pane is the one shown");
}

#[test]
fn a_replaced_pane_changes_nothing_and_is_a_reveal_failure() {
    let Some(mut live) = Live::start("stale") else {
        return;
    };
    live.session("ui");
    live.session("work");
    live.attach("ui");
    let (ui_pane, _) = live.pane_of("ui");
    let (pane, pid) = live.pane_of("work");
    let env = live.env_of(&ui_pane, true);
    let outcome = this_machine().switch(
        &target(&live, &pane, pid + 1, "work"),
        &env,
        &mut never_remote,
    );
    assert!(
        matches!(outcome, Err(SwitchError::Reveal(_))),
        "{outcome:?}"
    );
    assert_eq!(client_session(&live), "ui");
}

#[test]
fn two_terminals_refuse_and_nothing_moves() {
    let Some(mut live) = Live::start("refuse") else {
        return;
    };
    live.session("ui");
    live.session("work");
    live.attach("ui");
    live.attach("ui");
    let (ui_pane, _) = live.pane_of("ui");
    let (pane, pid) = live.pane_of("work");
    let env = live.env_of(&ui_pane, true);
    let outcome =
        this_machine().switch(&target(&live, &pane, pid, "work"), &env, &mut never_remote);
    assert!(
        matches!(outcome, Err(SwitchError::Refused(_))),
        "{outcome:?}"
    );
    assert_eq!(
        client_session(&live).lines().collect::<Vec<_>>(),
        ["ui", "ui"]
    );
}

#[test]
fn outside_tmux_the_dashboard_hands_over_to_a_local_attach() {
    let Some(live) = Live::start("attach") else {
        return;
    };
    live.session("work");
    let (pane, pid) = live.pane_of("work");
    let outcome = this_machine().switch(
        &target(&live, &pane, pid, "work"),
        &TmuxEnv::default(),
        &mut never_remote,
    );
    let Ok(Presented::Attach(handoff)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(handoff.program, "tmux");
    assert_eq!(
        handoff.args,
        [
            "-u",
            "-L",
            live.name.as_str(),
            "attach-session",
            "-t",
            "=work"
        ]
    );
}
