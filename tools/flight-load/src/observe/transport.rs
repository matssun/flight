// SPDX-License-Identifier: MIT

/// One pane as listed: its id, the pid of its process (so a reused `%id` is a new pane) and
/// the epoch second of its window's last activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    pub id: String,
    pub pid: u64,
    pub activity: u64,
}

/// How the observer talks to tmux. Strategies differ only in this and in the policy that
/// decides which panes to capture.
pub trait Transport {
    fn list(&mut self) -> Result<Vec<PaneRow>, String>;

    /// Visible screens (ANSI kept, last 50 lines) of `ids`, in the same order. A pane whose
    /// capture failed is `None`: the caller must not assume anything about it.
    fn capture(&mut self, ids: &[String]) -> Result<Vec<Option<String>>, String>;

    /// Re-establish the transport after an error (reconnect, re-attach). Stateless
    /// transports have nothing to do.
    fn recover(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// The columns requested from `list-panes`.
pub const LIST_FORMAT: &str = "#{pane_id}\t#{pane_pid}\t#{window_activity}";

/// The arguments of one `capture-pane` (what the node issues today).
pub fn capture_args(id: &str) -> [&str; 7] {
    ["capture-pane", "-p", "-e", "-S", "-50", "-t", id]
}

/// Parses a `list-panes` reply. A line that does not parse is an error, never "no pane": a
/// silently shortened list (the locale bug that once made a node report 0 panes) must not
/// look like an empty server.
pub fn parse_list(text: &str) -> Result<Vec<PaneRow>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let bad = || format!("unparseable pane list line {l:?}");
            let mut cols = l.split('\t');
            let id = cols
                .next()
                .filter(|id| id.starts_with('%'))
                .ok_or_else(bad)?;
            let pid = cols
                .next()
                .and_then(|c| c.trim().parse().ok())
                .ok_or_else(bad)?;
            let activity = cols
                .next()
                .and_then(|c| c.trim().parse().ok())
                .ok_or_else(bad)?;
            Ok(PaneRow {
                id: id.to_owned(),
                pid,
                activity,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows() {
        let rows = parse_list("%1\t100\t1700\n%2\t101\t1701\n").unwrap();
        assert_eq!(
            rows,
            vec![
                PaneRow {
                    id: "%1".into(),
                    pid: 100,
                    activity: 1700
                },
                PaneRow {
                    id: "%2".into(),
                    pid: 101,
                    activity: 1701
                },
            ]
        );
    }

    #[test]
    fn empty_is_empty_but_garbage_is_an_error() {
        assert_eq!(parse_list("").unwrap(), vec![]);
        // The separators rewritten to `_`, as tmux does without a UTF-8 locale.
        assert!(parse_list("%1_100_1700\n").is_err());
        assert!(parse_list("%1\t100\n").is_err());
        assert!(parse_list("%1\tx\t5\n").is_err());
        assert!(parse_list("%1\t100\t5\n%2\tbroken\n").is_err());
    }
}
