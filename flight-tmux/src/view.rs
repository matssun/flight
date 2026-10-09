// SPDX-License-Identifier: MIT

/// The prefix of the sessions Flight makes to show one surface of a workspace to one terminal
/// (ADR-009). A view is a member of its workspace's session group: it shares the workspace's
/// windows but has a current window of its own, so two terminals can show two surfaces of one
/// workspace at once and neither moves the other. The prefix is reserved; no session Flight
/// creates for a person is given a name that starts with it.
pub const VIEW_SESSION_PREFIX: &str = "flight-view-";

/// Whether `name` is the name of a view session.
pub fn is_view_session(name: &str) -> bool {
    name.starts_with(VIEW_SESSION_PREFIX)
}

/// The view session for the terminal with this id: unique to it, derived from nothing a peer
/// controls (the id is minted by the orchestrator and is 128 random bits).
pub fn view_session_name(terminal_id: &[u8]) -> String {
    let mut name = String::from(VIEW_SESSION_PREFIX);
    for byte in terminal_id.iter().take(8) {
        name.push_str(&format!("{byte:02x}"));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_name_is_a_view_and_a_person_s_session_is_not() {
        let name = view_session_name(&[0xab, 0x01, 0xff, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(name, "flight-view-ab01ff0001020304");
        assert!(is_view_session(&name));
        assert!(!is_view_session("flight"));
        assert!(!is_view_session("my-flight-view-x"));
    }

    #[test]
    fn different_terminals_have_different_views() {
        assert_ne!(view_session_name(&[1; 16]), view_session_name(&[2; 16]));
    }
}
