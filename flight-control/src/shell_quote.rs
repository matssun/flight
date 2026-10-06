// SPDX-License-Identifier: MIT

/// Quote one argument for a POSIX shell: ssh joins the remote command into a single string
/// that the remote shell re-parses, so every tmux argument (formats full of `#{...}` and
/// tabs, session targets) must be quoted.
pub(crate) fn shell_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn wraps_in_single_quotes() {
        assert_eq!(shell_quote("list-panes"), "'list-panes'");
        assert_eq!(shell_quote("#{pane_id}\t#{x}"), "'#{pane_id}\t#{x}'");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn neutralizes_shell_metacharacters() {
        assert_eq!(shell_quote("$(rm -rf /); a|b"), "'$(rm -rf /); a|b'");
    }
}
