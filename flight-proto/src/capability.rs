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
pub const CREATE_SESSION: &str = "create_session";

/// Every capability this build knows about.
pub const KNOWN: [&str; 6] = [
    PREVIEW,
    GUARDED_REVEAL,
    TERMINAL,
    SEND_INPUT,
    KILL,
    CREATE_SESSION,
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
