// SPDX-License-Identifier: MIT

use crate::{HostError, Transport};
use flight_tmux::TmuxError;
use std::io::ErrorKind;

/// ssh exits 255 for its own failures (connection, auth), and otherwise returns the remote
/// command's status.
const SSH_OWN_FAILURE: i32 = 255;
/// A POSIX shell's "command not found".
const COMMAND_NOT_FOUND: i32 = 127;

const AUTH_MARKERS: [&str; 3] = [
    "permission denied",
    "host key verification failed",
    "too many authentication failures",
];
const NO_SERVER_MARKERS: [&str; 4] = [
    "no server running",
    "error connecting to",
    "failed to connect to server",
    // What a client says when the server goes away while it is talking to it: the same fact,
    // seen a moment later (for example a list right after a kill).
    "server exited unexpectedly",
];
const NOT_FOUND_MARKERS: [&str; 2] = ["command not found", "not found"];

/// Map a tmux-level failure on `transport` to a typed host error.
pub fn classify(transport: &Transport, err: &TmuxError) -> HostError {
    match err {
        TmuxError::InvalidEndpoint(n) => HostError::InvalidConfig(format!("socket name {n:?}")),
        TmuxError::Spawn(e) => classify_spawn(transport, e),
        TmuxError::Failed { code, stderr } => classify_failed(transport, *code, stderr),
        TmuxError::Unparseable(_) | TmuxError::Control(_) => HostError::RemoteCommandFailed {
            code: None,
            stderr: err.to_string(),
        },
    }
}

fn classify_spawn(transport: &Transport, e: &std::io::Error) -> HostError {
    match (transport, e.kind()) {
        (Transport::Local, ErrorKind::NotFound) => HostError::TmuxUnavailable,
        _ => HostError::HostUnreachable {
            detail: format!("cannot run command: {e}"),
        },
    }
}

fn classify_failed(transport: &Transport, code: Option<i32>, stderr: &str) -> HostError {
    let lower = stderr.to_lowercase();
    let has = |markers: &[&str]| markers.iter().any(|m| lower.contains(m));
    if matches!(transport, Transport::Ssh { .. }) && code == Some(SSH_OWN_FAILURE) {
        let detail = stderr.to_owned();
        return if has(&AUTH_MARKERS) {
            HostError::AuthenticationFailed { detail }
        } else {
            HostError::HostUnreachable { detail }
        };
    }
    if has(&NO_SERVER_MARKERS) {
        return HostError::TmuxServerUnavailable;
    }
    if code == Some(COMMAND_NOT_FOUND) || (has(&NOT_FOUND_MARKERS) && lower.contains("tmux")) {
        return HostError::TmuxUnavailable;
    }
    HostError::RemoteCommandFailed {
        code,
        stderr: stderr.to_owned(),
    }
}

#[cfg(test)]
mod tests;
