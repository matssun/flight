// SPDX-License-Identifier: MIT

use std::collections::HashSet;

/// One pane as listed: its id and the epoch second of its window's last activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    pub id: String,
    pub activity: u64,
}

/// How the observer talks to tmux. Strategies differ only in this and in the policy that
/// decides which panes to capture.
pub trait Transport {
    fn list(&mut self) -> Result<Vec<PaneRow>, String>;

    /// Visible screens (ANSI kept, last 50 lines) of `ids`, in the same order.
    fn capture(&mut self, ids: &[String]) -> Result<Vec<String>, String>;

    /// Panes that produced output since the previous call, if this transport can tell.
    fn take_dirty(&mut self) -> Option<HashSet<String>> {
        None
    }
}

/// The columns requested from `list-panes`.
pub const LIST_FORMAT: &str = "#{pane_id}\t#{window_activity}";

/// The arguments of one `capture-pane` (what the node issues today).
pub fn capture_args(id: &str) -> [&str; 7] {
    ["capture-pane", "-p", "-e", "-S", "-50", "-t", id]
}

pub fn parse_list(text: &str) -> Vec<PaneRow> {
    text.lines()
        .filter_map(|l| {
            let (id, activity) = l.split_once('\t')?;
            Some(PaneRow {
                id: id.to_owned(),
                activity: activity.trim().parse().ok()?,
            })
        })
        .collect()
}
