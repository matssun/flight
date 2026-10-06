// SPDX-License-Identifier: MIT

use crate::{classify, HostError, HostStatus, Transport};
use flight_tmux::{TmuxError, TmuxRunner};

/// Probe a host in three steps, stopping at the first that tells us something is wrong:
/// can we run tmux at all (`-V`), and is a server up on the endpoint (`list-sessions`).
pub(crate) fn probe(transport: &Transport, runner: &dyn TmuxRunner) -> HostStatus {
    let version = match runner.run(&["-V"]) {
        Ok(out) => out.stdout.trim().to_owned(),
        Err(e) => return first_step_failed(transport, &e),
    };
    let base = HostStatus {
        reachable: true,
        tmux_available: true,
        endpoint_available: false,
        tmux_version: Some(version),
        problem: None,
    };
    match runner.run(&["list-sessions"]) {
        Ok(_) => HostStatus {
            endpoint_available: true,
            ..base
        },
        Err(e) => HostStatus {
            problem: Some(classify(transport, &e)),
            ..base
        },
    }
}

fn first_step_failed(transport: &Transport, e: &TmuxError) -> HostStatus {
    let problem = classify(transport, e);
    let reachable = !matches!(
        problem,
        HostError::HostUnreachable { .. } | HostError::AuthenticationFailed { .. }
    );
    HostStatus {
        reachable,
        tmux_available: false,
        endpoint_available: false,
        tmux_version: None,
        problem: Some(problem),
    }
}
