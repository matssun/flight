// SPDX-License-Identifier: MIT

//! Capabilities are named strings. Unknown names are not an error: they are negotiated away.

pub const PREVIEW: &str = "preview";
/// Reveal a pane, guarded by the pane process the caller saw (`RevealPane.expected_pid`).
/// The name carries the semantics: a peer that lacks it is refused, never given an
/// unguarded older behaviour.
pub const GUARDED_REVEAL: &str = "guarded_reveal_v1";
/// Open an interactive terminal session onto a pane (a PTY on the node running a tmux client),
/// guarded by the pane process the caller saw. ADR-003.
pub const TERMINAL: &str = "terminal_v1";
pub const SEND_INPUT: &str = "send_input";
pub const KILL: &str = "kill";
/// Create a session from a typed request: host, name, directory and a program from a closed
/// set. The name carries the semantics. The earlier `create_session` carried a free-form
/// command line and is retired: a node that still offers it must never be sent a request
/// whose program it would silently ignore.
pub const CREATE_SESSION: &str = "create_session_v1";

/// Create a surface (a companion shell) for an existing workspace, named by workspace id alone:
/// the node resolves the host's directory and session from the workspace itself.
pub const CREATE_SURFACE: &str = "create_surface_v1";

/// Every capability this build knows about.
pub const KNOWN: [&str; 7] = [
    PREVIEW,
    GUARDED_REVEAL,
    TERMINAL,
    SEND_INPUT,
    KILL,
    CREATE_SESSION,
    CREATE_SURFACE,
];

/// The offered capabilities that `supported` also contains, in offered order, without
/// duplicates. Anything unknown to this side is dropped silently.
pub fn negotiate(offered: &[String], supported: &[&str]) -> Vec<String> {
    let mut accepted: Vec<String> = Vec::new();
    for name in offered {
        if supported.contains(&name.as_str()) && !accepted.contains(name) {
            accepted.push(name.clone());
        }
    }
    accepted
}
