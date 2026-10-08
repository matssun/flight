// SPDX-License-Identifier: MIT

//! The remote path: reveal through the orchestrator first, then an ssh attach built only from
//! the operator's mapping. A failure says which stage it was.

use flight_client::{
    Presented, Refusal, SshDestinations, SwitchError, SwitchTarget, Switcher, TmuxEnv,
};
use flight_state::{HostId, PaneId, PaneRef, ServerId};
use std::cell::RefCell;

fn have_ssh() -> bool {
    std::process::Command::new("ssh").arg("-V").output().is_ok()
}

fn target(host: &str) -> SwitchTarget {
    SwitchTarget {
        pane: PaneRef {
            host: HostId::new(host),
            server: ServerId::new("flight"),
            pane: PaneId::new("%7"),
        },
        pid: 4321,
        session: "api".to_owned(),
    }
}

fn switcher(config: &str) -> Switcher {
    Switcher::new(
        Some(HostId::new("this-node")),
        SshDestinations::parse(config).unwrap(),
    )
}

const MINI: &str = "[nodes.\"mini-id\"]\nssh = \"mini-2\"\nname = \"anything\"\n";

#[test]
fn the_remote_pane_is_revealed_with_the_pid_the_user_saw_then_ssh_is_handed_over() {
    if !have_ssh() {
        return;
    }
    let seen = RefCell::new(Vec::new());
    let outcome = switcher(MINI).switch(&target("mini-id"), &TmuxEnv::default(), &mut |p, pid| {
        seen.borrow_mut().push((p.clone(), pid));
        Ok(())
    });
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(seen.borrow()[0].1, 4321);
    assert_eq!(seen.borrow()[0].0, target("mini-id").pane);
    let Ok(Presented::Attach(h)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(h.program, "ssh");
    assert_eq!(
        h.args,
        [
            "-t",
            "-o",
            "ConnectTimeout=5",
            "--",
            "mini-2",
            "'tmux' '-u' '-L' 'flight' 'attach-session' '-t' '=api'"
        ]
    );
}

#[test]
fn a_refused_reveal_stops_before_anything_is_presented() {
    if !have_ssh() {
        return;
    }
    let outcome = switcher(MINI).switch(&target("mini-id"), &TmuxEnv::default(), &mut |_, _| {
        Err("the pane changed since it was listed; refresh".to_owned())
    });
    assert_eq!(
        outcome,
        Err(SwitchError::Reveal(
            "the pane changed since it was listed; refresh".to_owned()
        ))
    );
}

#[test]
fn a_missing_mapping_is_refused_before_the_node_is_asked_to_do_anything() {
    let outcome = switcher("").switch(&target("mini-id"), &TmuxEnv::default(), &mut |_, _| {
        panic!("nothing may be revealed when the pane could not be shown anyway")
    });
    assert_eq!(
        outcome,
        Err(SwitchError::Refused(Refusal::NoSshDestination(
            HostId::new("mini-id")
        )))
    );
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("mini-id") && message.contains("ssh.toml"),
        "{message}"
    );
}

#[test]
fn a_node_that_renames_itself_gets_no_new_route() {
    // The advertised name is not a key: only the node id selects the destination.
    let config = "[nodes.\"prod-db\"]\nssh = \"prod-db-host\"\n";
    let outcome = switcher(config).switch(
        &target("attacker-node"),
        &TmuxEnv::default(),
        &mut |_, _| panic!("not routed"),
    );
    assert!(matches!(
        outcome,
        Err(SwitchError::Refused(Refusal::NoSshDestination(_)))
    ));
}

#[test]
fn a_session_name_cannot_break_out_of_the_remote_command() {
    if !have_ssh() {
        return;
    }
    let mut t = target("mini-id");
    t.session = "x'; touch /tmp/pwned; '".to_owned();
    let Ok(Presented::Attach(h)) =
        switcher(MINI).switch(&t, &TmuxEnv::default(), &mut |_, _| Ok(()))
    else {
        panic!()
    };
    let remote = h.args.last().unwrap();
    assert!(
        remote.ends_with(r"'=x'\''; touch /tmp/pwned; '\'''"),
        "{remote}"
    );
}

#[test]
fn present_and_reveal_failures_read_differently() {
    let reveal = SwitchError::Reveal("no pane".to_owned()).to_string();
    let present = SwitchError::Present("ssh: not found".to_owned()).to_string();
    assert!(reveal.starts_with("could not select"), "{reveal}");
    assert!(
        present.starts_with("pane selected, but cannot attach"),
        "{present}"
    );
}
