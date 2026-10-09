// SPDX-License-Identifier: MIT

use crate::{is_view_session, PaneInfo, TmuxError};
use std::collections::HashMap;

/// Format string for `list-panes -F`. `pane_title` stays LAST so a tab inside a
/// title cannot shift the other fields.
pub const PANE_FORMAT: &str = "#{pane_id}\t#{session_name}\t#{window_name}\t#{window_id}\t#{window_index}\t#{pane_current_path}\t#{pane_pid}\t#{pane_active}\t#{window_active}\t#{session_attached}\t#{pane_current_command}\t#{window_activity}\t#{?@flight_session,1,0}\t#{@flight_workspace}\t#{@flight_surface_id}\t#{@flight_surface}\t#{session_path}\t#{session_id}\t#{@flight_config}\t#{@flight_config_surface}\t#{pane_title}";

const FIELDS: usize = 21;

/// Parse `list-panes -F PANE_FORMAT` output, failing when panes were listed but none could be
/// read (say so rather than report "no panes").
pub fn parse_panes_checked(stdout: &str) -> Result<Vec<PaneInfo>, TmuxError> {
    let panes = parse_panes_output(stdout);
    if panes.is_empty() {
        if let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) {
            return Err(TmuxError::Unparseable(line.chars().take(80).collect()));
        }
    }
    Ok(panes)
}

/// Parse `list-panes -F PANE_FORMAT` output. Malformed lines are skipped.
///
/// A pane in a session group is listed once per session of the group, and Flight's view
/// sessions are in their workspace's group. Each pane comes out once, as its workspace's own
/// session lists it; a view only adds to whether the pane is being looked at.
pub fn parse_panes_output(stdout: &str) -> Vec<PaneInfo> {
    fold_views(stdout.lines().filter_map(parse_line).collect())
}

fn fold_views(rows: Vec<PaneInfo>) -> Vec<PaneInfo> {
    if !rows.iter().any(|r| is_view_session(&r.session_name)) {
        return rows;
    }
    let mut panes: Vec<PaneInfo> = Vec::with_capacity(rows.len());
    let mut index: HashMap<String, usize> = HashMap::new();
    for row in rows.iter().filter(|r| !is_view_session(&r.session_name)) {
        index.insert(row.pane_id.clone(), panes.len());
        panes.push(row.clone());
    }
    // How many clients are looking at each pane, counted over every session that shows it.
    let mut looking: HashMap<String, u32> = HashMap::new();
    for row in &rows {
        if row.pane_active && row.window_active {
            let n = looking.entry(row.pane_id.clone()).or_default();
            *n = n.saturating_add(row.session_attached);
        }
    }
    for row in rows.iter().filter(|r| is_view_session(&r.session_name)) {
        // A view whose own session is gone is the only listing there is.
        if !index.contains_key(&row.pane_id) {
            index.insert(row.pane_id.clone(), panes.len());
            panes.push(row.clone());
        }
    }
    for pane in &mut panes {
        let shown = looking.get(&pane.pane_id).copied().unwrap_or(0);
        if shown > 0 {
            pane.pane_active = true;
            pane.window_active = true;
            pane.session_attached = shown;
            pane.focused = true;
        }
    }
    panes
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
    // Unknown (0) rather than a parse failure: the pane is still listed, observers just
    // cannot rely on its activity.
    let window_activity = it.next()?.trim().parse().unwrap_or(0);
    let flight_session = it.next()? == "1";
    let workspace_id = it.next()?.to_owned();
    let surface_id = it.next()?.to_owned();
    let surface_kind = it.next()?.to_owned();
    let session_path = it.next()?.to_owned();
    let session_id = it.next()?.to_owned();
    let config_key = it.next()?.to_owned();
    let config_surface = it.next()?.to_owned();
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
        pane_active,
        window_active,
        session_attached: attached,
        window_activity,
        current_command,
        flight_session,
        pane_title,
        workspace_id,
        surface_id,
        surface_kind,
        session_path,
        session_id,
        config_key,
        config_surface,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(title: &str, active: &str, attached: &str) -> String {
        format!(
            "%3\tapi\tbuild\t@5\t2\t/tmp/x\t99\t{active}\t1\t{attached}\tclaude\t1700\t0\t\t\t\t/tmp/x\t$1\t\t\t{title}"
        )
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

    fn row(session: &str, pane_active: &str, window_active: &str, attached: &str) -> String {
        format!(
            "%3\t{session}\tbuild\t@5\t2\t/tmp/x\t99\t{pane_active}\t{window_active}\t{attached}\tclaude\t1700\t0\t\t\t\t/tmp/x\t$1\t\t\tt"
        )
    }

    #[test]
    fn a_pane_listed_by_its_workspace_and_by_a_view_comes_out_once_as_the_workspace_s() {
        let out = format!(
            "{}\n{}\n",
            row("api", "0", "0", "0"),
            row("flight-view-ab", "1", "1", "1")
        );
        let panes = parse_panes_output(&out);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].session_name, "api");
        // Someone is looking at it through the view, so it is focused.
        assert!(panes[0].focused);
        assert!(panes[0].focused_excluding(0));
        assert!(!panes[0].focused_excluding(1));
    }

    #[test]
    fn a_view_on_another_window_does_not_focus_the_pane() {
        let out = format!(
            "{}\n{}\n",
            row("api", "1", "1", "0"),
            row("flight-view-ab", "1", "0", "1")
        );
        let panes = parse_panes_output(&out);
        assert_eq!(panes.len(), 1);
        assert!(!panes[0].focused);
    }

    #[test]
    fn clients_looking_through_the_workspace_and_through_views_add_up() {
        let out = format!(
            "{}\n{}\n{}\n",
            row("api", "1", "1", "1"),
            row("flight-view-ab", "1", "1", "1"),
            row("flight-view-cd", "1", "1", "1")
        );
        let panes = parse_panes_output(&out);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].session_attached, 3);
        assert!(panes[0].focused_excluding(2));
    }

    #[test]
    fn a_view_whose_workspace_session_is_gone_is_still_listed_once() {
        let out = format!(
            "{}\n{}\n",
            row("flight-view-ab", "1", "1", "1"),
            row("flight-view-cd", "1", "0", "1")
        );
        let panes = parse_panes_output(&out);
        assert_eq!(panes.len(), 1);
        assert!(panes[0].focused);
    }

    #[test]
    fn a_pane_of_a_session_without_views_is_untouched() {
        let out = row("api", "1", "0", "2");
        let panes = parse_panes_output(&out);
        assert_eq!(panes.len(), 1);
        assert!(!panes[0].focused);
        assert_eq!(panes[0].session_attached, 2);
    }

    #[test]
    fn not_focused_when_no_client_attached() {
        assert!(!parse_panes_output(&line("t", "1", "0"))[0].focused);
    }

    #[test]
    fn activity_is_parsed_and_unknown_activity_is_zero() {
        assert_eq!(
            parse_panes_output(&line("t", "1", "1"))[0].window_activity,
            1700
        );
        let odd = line("t", "1", "1").replace("\t1700\t", "\t\t");
        assert_eq!(parse_panes_output(&odd)[0].window_activity, 0);
    }

    #[test]
    fn our_own_client_can_be_discounted_from_focus() {
        let p = &parse_panes_output(&line("t", "1", "1"))[0];
        assert!(p.focused && p.focused_excluding(0));
        assert!(!p.focused_excluding(1));
        let two = &parse_panes_output(&line("t", "1", "2"))[0];
        assert!(two.focused_excluding(1));
    }

    #[test]
    fn workspace_markers_and_the_session_root_are_read() {
        let marked = "%3\tapi\tshell\t@5\t2\t/tmp/x/sub\t99\t1\t1\t0\tzsh\t1700\t1\tw-1\ts-2\tshell\t/tmp/x\t$4\t\t\tt";
        let p = &parse_panes_output(marked)[0];
        assert_eq!(
            (
                p.workspace_id.as_str(),
                p.surface_id.as_str(),
                p.surface_kind.as_str(),
                p.session_path.as_str(),
                p.session_id.as_str()
            ),
            ("w-1", "s-2", "shell", "/tmp/x", "$4")
        );
        let unmarked = &parse_panes_output(&line("t", "1", "1"))[0];
        assert!(unmarked.workspace_id.is_empty() && unmarked.surface_kind.is_empty());
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
