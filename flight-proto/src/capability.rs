// SPDX-License-Identifier: MIT

//! Capabilities are named strings. Unknown names are not an error: they are negotiated away.

pub const PREVIEW: &str = "preview";
pub const SWITCH: &str = "switch";
pub const SEND_INPUT: &str = "send_input";
pub const KILL: &str = "kill";
pub const CREATE_SESSION: &str = "create_session";

/// Every capability this build knows about.
pub const KNOWN: [&str; 5] = [PREVIEW, SWITCH, SEND_INPUT, KILL, CREATE_SESSION];

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
