// SPDX-License-Identifier: MIT

//! Routing, typed failures and status probing against scripted runners (no tmux, no ssh).

use flight_control::{BoxedRunner, HostError, HostRegistry, Transport};
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use flight_tmux::{TmuxEndpoint, TmuxError, TmuxOutput, TmuxRunner};
use std::sync::Mutex;

type Reply = Result<String, (Option<i32>, String)>;

/// Replies by first tmux argument; records every call.
struct Script {
    replies: Vec<(&'static str, Reply)>,
    calls: std::sync::Arc<Mutex<Vec<Vec<String>>>>,
}

impl TmuxRunner for Script {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        self.calls
            .lock()
            .unwrap()
            .push(args.iter().map(|s| (*s).to_owned()).collect());
        let first = args.first().copied().unwrap_or("");
        let reply = self
            .replies
            .iter()
            .find(|(k, _)| *k == first)
            .map(|(_, r)| r.clone());
        match reply {
            Some(Ok(stdout)) => Ok(TmuxOutput { stdout }),
            Some(Err((code, stderr))) => Err(TmuxError::Failed { code, stderr }),
            None => Err(TmuxError::Failed {
                code: Some(1),
                stderr: format!("unscripted: {args:?}"),
            }),
        }
    }
}

fn host(s: &str) -> HostId {
    HostId::new(s)
}

fn server() -> ServerId {
    ServerId::new("flight")
}

fn registry_with(
    transport: Transport,
    replies: Vec<(&'static str, Reply)>,
) -> (HostRegistry, std::sync::Arc<Mutex<Vec<Vec<String>>>>) {
    let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
    let mut r = HostRegistry::new();
    r.add_host(host("h"), transport);
    let runner: BoxedRunner = Box::new(Script {
        replies,
        calls: calls.clone(),
    });
    r.add_server_with_runner(
        &host("h"),
        server(),
        TmuxEndpoint::named("flight").unwrap(),
        runner,
    )
    .unwrap();
    (r, calls)
}

fn pane_line(id: &str, session: &str) -> String {
    format!("{id}\t{session}\tw\t@1\t0\t/tmp\t7\t1\t1\t1\tzsh\t1700\t0\t\t\t\t/tmp\t$1\t\t\ttitle\n")
}

fn ssh() -> Transport {
    Transport::Ssh {
        alias: "mini-2".into(),
    }
}

#[test]
fn list_panes_attaches_host_and_server_identity() {
    let (r, _) = registry_with(
        Transport::Local,
        vec![("list-panes", Ok(pane_line("%1", "api")))],
    );
    let panes = r.list_panes(&host("h"), &server()).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(
        panes[0].pane_ref,
        PaneRef {
            host: host("h"),
            server: server(),
            pane: PaneId::new("%1")
        }
    );
    assert_eq!(panes[0].info.session_name, "api");
}

#[test]
fn same_pane_id_on_two_hosts_stays_distinct() {
    let mut r = HostRegistry::new();
    for h in ["a", "b"] {
        r.add_host(host(h), Transport::Local);
        let runner: BoxedRunner = Box::new(Script {
            replies: vec![("list-panes", Ok(pane_line("%1", "nga")))],
            calls: Default::default(),
        });
        r.add_server_with_runner(
            &host(h),
            server(),
            TmuxEndpoint::named("flight").unwrap(),
            runner,
        )
        .unwrap();
    }
    let all: Vec<_> = r
        .list_all_panes()
        .into_iter()
        .flat_map(|o| o.result.unwrap())
        .collect();
    assert_eq!(all.len(), 2);
    assert_ne!(all[0].pane_ref, all[1].pane_ref);
}

#[test]
fn one_down_host_does_not_hide_the_others() {
    let mut r = HostRegistry::new();
    r.add_host(host("up"), Transport::Local);
    r.add_host(host("down"), ssh());
    let up: BoxedRunner = Box::new(Script {
        replies: vec![("list-panes", Ok(pane_line("%1", "a")))],
        calls: Default::default(),
    });
    let down: BoxedRunner = Box::new(Script {
        replies: vec![(
            "list-panes",
            Err((
                Some(255),
                "ssh: connect to host mini-2 port 22: Connection refused".into(),
            )),
        )],
        calls: Default::default(),
    });
    r.add_server_with_runner(
        &host("up"),
        server(),
        TmuxEndpoint::named("flight").unwrap(),
        up,
    )
    .unwrap();
    r.add_server_with_runner(
        &host("down"),
        server(),
        TmuxEndpoint::named("flight").unwrap(),
        down,
    )
    .unwrap();
    let out = r.list_all_panes();
    let by = |h: &str| out.iter().find(|o| o.host == host(h)).unwrap();
    assert_eq!(by("up").result.as_ref().unwrap().len(), 1);
    assert!(matches!(
        by("down").result,
        Err(HostError::HostUnreachable { .. })
    ));
}

#[test]
fn unknown_host_and_server_are_typed() {
    let (r, _) = registry_with(Transport::Local, vec![]);
    assert_eq!(
        r.list_panes(&host("nope"), &server()),
        Err(HostError::UnknownHost(host("nope")))
    );
    assert_eq!(
        r.list_panes(&host("h"), &ServerId::new("other")),
        Err(HostError::UnknownServer(host("h"), ServerId::new("other")))
    );
    let pane = PaneRef {
        host: host("nope"),
        server: server(),
        pane: PaneId::new("%1"),
    };
    assert_eq!(
        r.kill_pane(&pane),
        Err(HostError::UnknownHost(host("nope")))
    );
}

#[test]
fn pane_operations_route_to_the_pane_s_own_host_and_target_its_pane() {
    let (r, calls) = registry_with(
        Transport::Local,
        vec![
            ("capture-pane", Ok("screen".into())),
            ("kill-pane", Ok(String::new())),
        ],
    );
    let pane = PaneRef {
        host: host("h"),
        server: server(),
        pane: PaneId::new("%9"),
    };
    assert_eq!(r.capture_pane(&pane, false, Some(10)).unwrap(), "screen");
    r.kill_pane(&pane).unwrap();
    let calls = calls.lock().unwrap();
    assert!(calls[0].contains(&"%9".to_owned()) && calls[0][0] == "capture-pane");
    assert_eq!(calls[1], ["kill-pane", "-t", "%9"]);
}

#[test]
fn failures_carry_the_typed_reason() {
    let (r, _) = registry_with(
        Transport::Local,
        vec![(
            "list-panes",
            Err((Some(1), "no server running on /tmp/x".into())),
        )],
    );
    assert_eq!(
        r.list_panes(&host("h"), &server()),
        Err(HostError::TmuxServerUnavailable)
    );
    let (r, _) = registry_with(
        ssh(),
        vec![(
            "list-panes",
            Err((Some(255), "Permission denied (publickey).".into())),
        )],
    );
    assert!(matches!(
        r.list_panes(&host("h"), &server()),
        Err(HostError::AuthenticationFailed { .. })
    ));
    let (r, _) = registry_with(
        ssh(),
        vec![("kill-pane", Err((Some(1), "can't find pane: %9".into())))],
    );
    let pane = PaneRef {
        host: host("h"),
        server: server(),
        pane: PaneId::new("%9"),
    };
    assert!(matches!(
        r.kill_pane(&pane),
        Err(HostError::RemoteCommandFailed { code: Some(1), .. })
    ));
}

#[test]
fn status_online() {
    let (r, _) = registry_with(
        Transport::Local,
        vec![
            ("-V", Ok("tmux 3.7b\n".into())),
            ("list-sessions", Ok("a: 1 windows\n".into())),
        ],
    );
    let s = r.status(&host("h"), &server()).unwrap();
    assert!(s.is_online());
    assert_eq!(
        (s.tmux_version.as_deref(), s.problem),
        (Some("tmux 3.7b"), None)
    );
}

#[test]
fn status_online_host_without_a_flight_server() {
    let (r, _) = registry_with(
        Transport::Local,
        vec![
            ("-V", Ok("tmux 3.7b".into())),
            (
                "list-sessions",
                Err((Some(1), "no server running on /tmp/x".into())),
            ),
        ],
    );
    let s = r.status(&host("h"), &server()).unwrap();
    assert_eq!(
        (s.reachable, s.tmux_available, s.endpoint_available),
        (true, true, false)
    );
    assert_eq!(s.problem, Some(HostError::TmuxServerUnavailable));
    assert!(!s.is_online());
}

#[test]
fn status_unreachable_host() {
    let (r, _) = registry_with(
        ssh(),
        vec![(
            "-V",
            Err((Some(255), "ssh: Could not resolve hostname mini-2".into())),
        )],
    );
    let s = r.status(&host("h"), &server()).unwrap();
    assert_eq!(
        (s.reachable, s.tmux_available, s.endpoint_available),
        (false, false, false)
    );
    assert!(matches!(s.problem, Some(HostError::HostUnreachable { .. })));
}

#[test]
fn status_reachable_host_without_tmux() {
    let (r, _) = registry_with(
        ssh(),
        vec![(
            "-V",
            Err((Some(127), "bash: tmux: command not found".into())),
        )],
    );
    let s = r.status(&host("h"), &server()).unwrap();
    assert_eq!((s.reachable, s.tmux_available), (true, false));
    assert_eq!(s.problem, Some(HostError::TmuxUnavailable));
}

#[test]
fn status_auth_failure_is_not_reachable_but_is_distinguishable() {
    let (r, _) = registry_with(
        ssh(),
        vec![(
            "-V",
            Err((Some(255), "Host key verification failed.".into())),
        )],
    );
    let s = r.status(&host("h"), &server()).unwrap();
    assert!(!s.reachable);
    assert!(matches!(
        s.problem,
        Some(HostError::AuthenticationFailed { .. })
    ));
}

#[test]
fn bad_ssh_alias_is_rejected_when_the_server_is_added() {
    let mut r = HostRegistry::new();
    r.add_host(
        host("h"),
        Transport::Ssh {
            alias: "-oProxyCommand=evil".into(),
        },
    );
    let e = r.add_server(&host("h"), server(), TmuxEndpoint::named("flight").unwrap());
    assert!(matches!(e, Err(HostError::InvalidConfig(_))));
}

#[test]
fn switching_to_a_remote_pane_is_unsupported_not_attempted() {
    let (r, calls) = registry_with(ssh(), vec![]);
    let pane = PaneRef {
        host: host("h"),
        server: server(),
        pane: PaneId::new("%1"),
    };
    assert!(matches!(
        r.switch_to_pane(&pane),
        Err(HostError::Unsupported(_))
    ));
    assert!(calls.lock().unwrap().is_empty(), "no ssh was attempted");
}

#[test]
fn switching_to_a_local_pane_targets_that_pane() {
    let (r, calls) = registry_with(Transport::Local, vec![("switch-client", Ok(String::new()))]);
    let pane = PaneRef {
        host: host("h"),
        server: server(),
        pane: PaneId::new("%4"),
    };
    r.switch_to_pane(&pane).unwrap();
    assert_eq!(calls.lock().unwrap()[0], ["switch-client", "-t", "%4"]);
}
