// SPDX-License-Identifier: MIT

use super::*;

fn ssh() -> Transport {
    Transport::Ssh {
        alias: "mini-2".into(),
    }
}

fn failed(code: i32, stderr: &str) -> TmuxError {
    TmuxError::Failed {
        code: Some(code),
        stderr: stderr.into(),
    }
}

#[test]
fn ssh_connection_failures_are_unreachable() {
    for s in [
        "ssh: Could not resolve hostname mini-9: nodename nor servname provided, or not known",
        "ssh: connect to host 10.0.0.9 port 22: Connection refused",
        "ssh: connect to host 10.0.0.9 port 22: Operation timed out",
    ] {
        assert!(
            matches!(
                classify(&ssh(), &failed(255, s)),
                HostError::HostUnreachable { .. }
            ),
            "{s}"
        );
    }
}

#[test]
fn ssh_auth_failures_are_distinct() {
    for s in [
        "mats@mini-2: Permission denied (publickey).",
        "Host key verification failed.",
    ] {
        assert!(
            matches!(
                classify(&ssh(), &failed(255, s)),
                HostError::AuthenticationFailed { .. }
            ),
            "{s}"
        );
    }
}

#[test]
fn a_server_that_goes_away_mid_command_is_a_missing_server() {
    // Seen on Linux CI: a list issued right after a kill reached the dying server.
    for transport in [Transport::Local, ssh()] {
        assert_eq!(
            classify(&transport, &failed(1, "server exited unexpectedly")),
            HostError::TmuxServerUnavailable
        );
    }
}

#[test]
fn missing_tmux_is_distinct_from_a_missing_server() {
    assert_eq!(
        classify(&ssh(), &failed(127, "bash: tmux: command not found")),
        HostError::TmuxUnavailable
    );
    assert_eq!(
        classify(&ssh(), &failed(127, "zsh:1: command not found: tmux")),
        HostError::TmuxUnavailable
    );
    assert_eq!(
        classify(
            &ssh(),
            &failed(1, "no server running on /tmp/tmux-501/flight")
        ),
        HostError::TmuxServerUnavailable
    );
    assert_eq!(
        classify(
            &Transport::Local,
            &failed(
                1,
                "error connecting to /private/tmp/tmux-501/flight (No such file or directory)"
            )
        ),
        HostError::TmuxServerUnavailable
    );
}

#[test]
fn other_tmux_failures_keep_their_detail() {
    let e = classify(&Transport::Local, &failed(1, "can't find pane: %99"));
    assert_eq!(
        e,
        HostError::RemoteCommandFailed {
            code: Some(1),
            stderr: "can't find pane: %99".into()
        }
    );
}

#[test]
fn exit_255_is_only_special_over_ssh() {
    assert!(matches!(
        classify(&Transport::Local, &failed(255, "boom")),
        HostError::RemoteCommandFailed { .. }
    ));
}

#[test]
fn local_spawn_failure_means_tmux_is_missing_but_ssh_spawn_failure_means_unreachable() {
    let nf = || TmuxError::Spawn(std::io::Error::from(ErrorKind::NotFound));
    assert_eq!(
        classify(&Transport::Local, &nf()),
        HostError::TmuxUnavailable
    );
    assert!(matches!(
        classify(&ssh(), &nf()),
        HostError::HostUnreachable { .. }
    ));
}
