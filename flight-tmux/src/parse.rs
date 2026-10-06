// SPDX-License-Identifier: MIT

use crate::PaneInfo;

/// Format string for `list-panes -F`. `pane_title` stays LAST so a tab inside a
/// title cannot shift the other fields.
pub const PANE_FORMAT: &str = "#{pane_id}\t#{session_name}\t#{window_name}\t#{window_id}\t#{window_index}\t#{pane_current_path}\t#{pane_pid}\t#{pane_active}\t#{window_active}\t#{session_attached}\t#{pane_current_command}\t#{pane_title}";

const FIELDS: usize = 12;

/// Parse `list-panes -F PANE_FORMAT` output. Malformed lines are skipped.
pub fn parse_panes_output(stdout: &str) -> Vec<PaneInfo> {
    stdout.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<PaneInfo> {
    let mut it = line.splitn(FIELDS, '\t');
    let pane_id = it.next()?.to_owned();
    let session_name = it.next()?.to_owned();
    let window_name = it.next()?.to_owned();
    let window_id = it.next()?.to_owned();
    let window_index = it.next()?.parse().ok()?;
    let current_path = it.next()?.to_owned();
    let pane_pid = it.next()?.parse().ok()?;
    let pane_active = it.next()? == "1";
    let window_active = it.next()? == "1";
    let attached: u32 = it.next()?.parse().ok()?;
    let current_command = it.next()?.to_owned();
    let pane_title = it.next()?.to_owned();
    Some(PaneInfo {
        pane_id,
        session_name,
        window_name,
        window_id,
        window_index,
        current_path,
        pane_pid,
        focused: pane_active && window_active && attached > 0,
        current_command,
        pane_title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(title: &str, active: &str, attached: &str) -> String {
        format!("%3\tapi\tbuild\t@5\t2\t/tmp/x\t99\t{active}\t1\t{attached}\tclaude\t{title}")
    }

    #[test]
    fn parses_one_pane() {
        let panes = parse_panes_output(&line("t", "1", "1"));
        assert_eq!(panes.len(), 1);
        let p = &panes[0];
        assert_eq!(
            (p.pane_id.as_str(), p.window_index, p.pane_pid),
            ("%3", 2, 99)
        );
        assert!(p.focused);
    }

    #[test]
    fn not_focused_when_no_client_attached() {
        assert!(!parse_panes_output(&line("t", "1", "0"))[0].focused);
    }

    #[test]
    fn tab_in_title_is_preserved() {
        assert_eq!(
            parse_panes_output(&line("a\tb", "0", "0"))[0].pane_title,
            "a\tb"
        );
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let out = format!("garbage\n{}\n\n%1\tonly\n", line("t", "0", "0"));
        assert_eq!(parse_panes_output(&out).len(), 1);
    }
}
