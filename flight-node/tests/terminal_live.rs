// SPDX-License-Identifier: MIT

//! Terminals against a real tmux server on a private socket (skipped without tmux): a PTY the
//! node owns runs one tmux client attached to the named pane, only if the pane still runs the
//! process the caller saw, and leaves nothing behind.

use flight_node::{
    tmux_attach_command, Control, OpenedTerminal, TerminalProcess, TerminalSpec, TmuxServers,
};
use flight_proto::ErrorKindCode;
use flight_state::{PaneId, ServerId};
use flight_tmux::{SystemRunner, Tmux, TmuxEndpoint, TmuxRunner};
use std::io::Read;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

struct Live {
    endpoint: TmuxEndpoint,
    tmux: Tmux,
    servers: TmuxServers,
}

impl Live {
    fn start(tag: &str) -> Option<Self> {
        Command::new("tmux").arg("-V").output().ok()?;
        let endpoint =
            TmuxEndpoint::named(&format!("flight-test-{}-{tag}", std::process::id())).ok()?;
        let mut servers = TmuxServers::new();
        servers.add(
            ServerId::new("live"),
            Box::new(SystemRunner::new(endpoint.clone())),
        );
        servers.allow_terminal(ServerId::new("live"), endpoint.clone());
        Some(Self {
            tmux: Tmux::new(endpoint.clone()),
            endpoint,
            servers,
        })
    }

    fn raw(&self, args: &[&str]) -> String {
        self.tmux.runner().run(args).unwrap().stdout
    }

    fn session(&self, name: &str) -> (String, u32) {
        self.raw(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            "100",
            "-y",
            "30",
            "cat",
        ]);
        let out = self.raw(&[
            "list-panes",
            "-t",
            &format!("={name}:"),
            "-F",
            "#{pane_id} #{pane_pid}",
        ]);
        let (id, pid) = out.trim().split_once(' ').unwrap();
        (id.to_owned(), pid.parse().unwrap())
    }

    fn clients(&self) -> Vec<String> {
        self.tmux
            .runner()
            .run(&[
                "list-clients",
                "-F",
                "#{client_session} #{client_width}x#{client_height}",
            ])
            .map(|o| o.stdout.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn wait_clients(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let c = self.clients();
            if c.len() == n {
                return c;
            }
            assert!(
                Instant::now() < deadline,
                "clients stayed at {c:?}, want {n}"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    fn spec(&self, pane: &str, pid: u32) -> TerminalSpec {
        TerminalSpec {
            request_id: 1,
            terminal_id: [9; 16],
            server: ServerId::new("live"),
            pane: PaneId::new(pane),
            pid,
            cols: 90,
            rows: 25,
            term: "xterm-256color".to_owned(),
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server();
        let _ = &self.endpoint;
    }
}

fn output_of(reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

fn wait_for(rx: &Receiver<Vec<u8>>, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = String::new();
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.push_str(&String::from_utf8_lossy(&chunk));
            if seen.contains(needle) {
                return seen;
            }
        }
    }
    panic!("never saw {needle:?} in {seen:?}");
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_terminal_attaches_one_client_to_the_pane_and_carries_input_output_and_size() {
    let Some(live) = Live::start("attach") else {
        return;
    };
    let (other_pane, other_pid) = live.session("other");
    let (pane, pid) = live.session("work");
    let OpenedTerminal {
        mut process,
        reader,
    } = live.servers.open_terminal(&live.spec(&pane, pid)).unwrap();
    let rx = output_of(reader);
    let clients = live.wait_clients(1);
    assert!(clients[0].starts_with("work "), "{clients:?}");
    assert_eq!(clients[0], "work 90x25");

    process.write_all(b"typed-through-flight\r").unwrap();
    wait_for(&rx, "typed-through-flight");
    let screen = live.raw(&["capture-pane", "-p", "-t", &pane]);
    assert!(screen.contains("typed-through-flight"), "{screen}");

    process.resize(70, 20).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while live.clients()[0] != "work 70x20" {
        assert!(Instant::now() < deadline, "{:?}", live.clients());
        std::thread::sleep(Duration::from_millis(30));
    }

    let child = process.process_id().unwrap();
    assert!(process.hang_up(Duration::from_secs(5)).is_some(), "reaped");
    live.wait_clients(0);
    assert!(!alive(child), "no client process is left behind");
    // The session and its program are untouched.
    live.raw(&["has-session", "-t", "=work"]);
    live.raw(&["has-session", "-t", "=other"]);
    let _ = (other_pane, other_pid);
}

#[test]
fn a_replaced_pane_creates_no_terminal() {
    let Some(live) = Live::start("stale") else {
        return;
    };
    let (pane, pid) = live.session("work");
    let outcome = live.servers.open_terminal(&live.spec(&pane, pid + 1));
    let Err(e) = outcome else {
        panic!("a stale terminal was opened")
    };
    assert_eq!(e.kind, ErrorKindCode::PaneChanged);
    std::thread::sleep(Duration::from_millis(300));
    assert!(live.clients().is_empty());
}

#[test]
fn unknown_panes_and_servers_without_terminals_are_typed_refusals() {
    let Some(live) = Live::start("refuse") else {
        return;
    };
    let (_, pid) = live.session("work");
    let Err(e) = live.servers.open_terminal(&live.spec("%999", pid)) else {
        panic!()
    };
    assert_eq!(e.kind, ErrorKindCode::UnknownPane);
    let mut plain = TmuxServers::new();
    plain.add(
        ServerId::new("live"),
        Box::new(SystemRunner::new(live.endpoint.clone())),
    );
    let (pane, pid) = live.session("again");
    let Err(e) = plain.open_terminal(&live.spec(&pane, pid)) else {
        panic!()
    };
    assert_eq!(e.kind, ErrorKindCode::Unsupported);
}

#[test]
fn tmux_itself_refuses_a_wrong_pid_in_the_attach_command() {
    // The window between the node's check and the attach does not exist: even if the node's
    // check were skipped, the command that attaches checks the pid again and attaches nothing.
    let Some(live) = Live::start("atomic") else {
        return;
    };
    let (pane, pid) = live.session("work");
    for (wrong, expect_client) in [(pid + 1, false), (pid, true)] {
        let args = tmux_attach_command(&live.endpoint, &pane, wrong);
        let OpenedTerminal {
            mut process,
            reader,
        } = TerminalProcess::spawn("tmux", &args, &env(), 80, 24).unwrap();
        let _rx = output_of(reader);
        if expect_client {
            live.wait_clients(1);
        } else {
            let deadline = Instant::now() + Duration::from_secs(10);
            let code = loop {
                if let Some(code) = process.try_exit_code() {
                    break code;
                }
                assert!(Instant::now() < deadline, "tmux never exited");
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!(code, 1, "a failed guard exits 1");
            assert!(live.clients().is_empty(), "nothing attached");
        }
        process.hang_up(Duration::from_secs(5));
    }
}

#[test]
fn dropping_a_terminal_leaves_no_client_and_no_process() {
    let Some(live) = Live::start("drop") else {
        return;
    };
    let (pane, pid) = live.session("work");
    let opened = live.servers.open_terminal(&live.spec(&pane, pid)).unwrap();
    let child = opened.process.process_id().unwrap();
    let _rx = output_of(opened.reader);
    live.wait_clients(1);
    drop(opened.process);
    live.wait_clients(0);
    assert!(!alive(child));
}

#[test]
fn the_terminal_ends_by_itself_when_the_user_detaches_in_tmux() {
    let Some(live) = Live::start("detach") else {
        return;
    };
    let (pane, pid) = live.session("work");
    let OpenedTerminal {
        mut process,
        reader,
    } = live.servers.open_terminal(&live.spec(&pane, pid)).unwrap();
    let rx = output_of(reader);
    live.wait_clients(1);
    live.raw(&["detach-client", "-s", "work"]);
    // End of file on the output, then a reapable exit.
    let deadline = Instant::now() + Duration::from_secs(10);
    while rx.recv_timeout(Duration::from_millis(100))
        != Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    {
        assert!(Instant::now() < deadline, "output never ended");
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while process.try_exit_code().is_none() {
        assert!(Instant::now() < deadline, "client never exited");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn env() -> Vec<(String, String)> {
    vec![
        ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".to_owned(), std::env::var("HOME").unwrap_or_default()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
    ]
}
