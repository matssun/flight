// SPDX-License-Identifier: MIT

/// The longest session name a request may carry.
/// (Shared by the dashboard form, the wire validation and the node, so all three agree.)
pub const MAX_SESSION_NAME_LEN: usize = 64;
/// The longest directory a request may carry (Linux `PATH_MAX`).
pub const MAX_DIR_LEN: usize = 4096;

/// Names starting with this are Flight's own view sessions (`flight_tmux::VIEW_SESSION_PREFIX`,
/// which a test in `flight-node` keeps equal) and are never given to a session a person makes.
pub const RESERVED_VIEW_PREFIX: &str = "flight-view-";

/// A session name: 1 to [`MAX_SESSION_NAME_LEN`] bytes of `[A-Za-z0-9_-]`, not starting with
/// `-`. tmux rewrites `.` and `:` in names and treats a leading `-` as an option, so none of
/// them is accepted; what a person types is what the session is called.
pub fn valid_session_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SESSION_NAME_LEN
        && !name.starts_with('-')
        && !name.starts_with(RESERVED_VIEW_PREFIX)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// A directory as it travels: non-empty, bounded, absolute or `~`-relative, with no control
/// characters (a NUL ends a C string; a newline can split a log line). Whether it exists is
/// for the node to say.
pub fn valid_dir(dir: &str) -> bool {
    dir.len() <= MAX_DIR_LEN
        && (dir.starts_with('/') || dir == "~" || dir.starts_with("~/"))
        && !dir.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in [
            "api",
            "my-app_2",
            "_x",
            "A",
            &"a".repeat(MAX_SESSION_NAME_LEN),
        ] {
            assert!(valid_session_name(ok), "{ok}");
        }
        for bad in [
            "",
            "-x",
            "a b",
            "a.b",
            "a:b",
            "a;b",
            "a\nb",
            "ä",
            "$(x)",
            "flight-view-ab12",
            &"a".repeat(MAX_SESSION_NAME_LEN + 1),
        ] {
            assert!(!valid_session_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn dirs() {
        for ok in ["/", "/srv/my app", "~", "~/dev/x"] {
            assert!(valid_dir(ok), "{ok}");
        }
        for bad in [
            "",
            "rel/x",
            "~root",
            "/a\0b",
            "/a\nb",
            &format!("/{}", "a".repeat(MAX_DIR_LEN)),
        ] {
            assert!(!valid_dir(bad), "{bad:?}");
        }
    }
}
