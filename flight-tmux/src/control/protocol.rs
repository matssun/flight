// SPDX-License-Identifier: MIT

/// One command's reply from a control-mode connection.
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub lines: Vec<String>,
    /// False when the reply closed with `%error`.
    pub ok: bool,
}

/// What one input line amounted to.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// Nothing to act on (a notification, or a line inside a reply).
    Nothing,
    Reply(Reply),
    /// The server ended the connection.
    Exit,
    /// The stream no longer makes sense; the connection must be rebuilt.
    Desync(String),
}

/// Notifications that may appear at any time, even between a `%begin` and its `%end`. They
/// are never part of a reply. (A reply line such as a pane id `%3` never starts with one.)
const NOTIFICATIONS: [&str; 12] = [
    "%output ",
    "%extended-output ",
    "%pause ",
    "%continue ",
    "%window-",
    "%unlinked-window-",
    "%linked-window-",
    "%layout-change ",
    "%session-",
    "%sessions-changed",
    "%client-",
    "%pane-mode-changed ",
];

/// Splits a control-mode stream into replies. Replies are matched to commands by order, so
/// the parser checks each `%end`/`%error` against its `%begin` and reports a mismatch as a
/// desync instead of attributing text to the wrong command.
#[derive(Default)]
pub struct Parser {
    open: Option<Open>,
}

struct Open {
    command: String,
    lines: Vec<String>,
}

fn command_number(line: &str) -> String {
    // `%begin <time> <number> <flags>`
    line.split(' ').nth(2).unwrap_or_default().to_owned()
}

impl Parser {
    pub fn feed(&mut self, line: &str) -> Event {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if NOTIFICATIONS.iter().any(|n| line.starts_with(n)) {
            return Event::Nothing;
        }
        match self.open.as_mut() {
            Some(open) if line.starts_with("%end ") || line.starts_with("%error ") => {
                if command_number(line) != open.command {
                    let message = format!("{line:?} closes command {}", open.command);
                    self.open = None;
                    return Event::Desync(message);
                }
                let ok = line.starts_with("%end ");
                let lines = std::mem::take(&mut open.lines);
                self.open = None;
                Event::Reply(Reply { lines, ok })
            }
            Some(_) if line.starts_with("%begin ") => {
                self.open = None;
                Event::Desync("%begin inside a reply".to_owned())
            }
            Some(open) => {
                open.lines.push(line.to_owned());
                Event::Nothing
            }
            None if line.starts_with("%begin ") => {
                self.open = Some(Open {
                    command: command_number(line),
                    lines: Vec::new(),
                });
                Event::Nothing
            }
            None if line.starts_with("%exit") => Event::Exit,
            None if line.starts_with("%end ") || line.starts_with("%error ") => {
                Event::Desync(format!("{line:?} without %begin"))
            }
            None => Event::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(lines: &[&str]) -> Vec<Event> {
        let mut p = Parser::default();
        lines.iter().map(|l| p.feed(l)).collect()
    }

    fn reply(lines: &[&str], ok: bool) -> Event {
        Event::Reply(Reply {
            lines: lines.iter().map(|s| (*s).to_owned()).collect(),
            ok,
        })
    }

    #[test]
    fn a_reply_collects_its_lines() {
        let ev = feed_all(&["%begin 1 7 1", "%1\t10\t5", "%2\t11\t6", "%end 1 7 1"]);
        assert_eq!(ev[3], reply(&["%1\t10\t5", "%2\t11\t6"], true));
    }

    #[test]
    fn output_interleaved_inside_a_reply_is_not_reply_text() {
        let ev = feed_all(&[
            "%begin 1 7 1",
            "line a",
            "%output %4 hello",
            "line b",
            "%extended-output %4 3 : x",
            "%end 1 7 1",
        ]);
        assert_eq!(ev[5], reply(&["line a", "line b"], true));
    }

    #[test]
    fn pane_id_lines_are_reply_text_not_notifications() {
        let ev = feed_all(&["%begin 1 7 1", "%3\t10\t5", "%end 1 7 1"]);
        assert_eq!(ev[2], reply(&["%3\t10\t5"], true));
    }

    #[test]
    fn error_replies_are_marked() {
        let ev = feed_all(&["%begin 1 8 1", "can't find pane: %9", "%error 1 8 1"]);
        assert_eq!(ev[2], reply(&["can't find pane: %9"], false));
    }

    #[test]
    fn notifications_between_replies_are_ignored() {
        let ev = feed_all(&[
            "%window-add @3",
            "%sessions-changed",
            "%begin 1 1 1",
            "%end 1 1 1",
        ]);
        assert_eq!(ev[0], Event::Nothing);
        assert_eq!(ev[1], Event::Nothing);
        assert_eq!(ev[3], reply(&[], true));
    }

    #[test]
    fn mismatched_or_unopened_replies_are_desyncs() {
        let ev = feed_all(&["%begin 1 7 1", "x", "%end 1 8 1"]);
        assert!(matches!(ev[2], Event::Desync(_)));
        assert!(matches!(feed_all(&["%end 1 7 1"])[0], Event::Desync(_)));
        assert!(matches!(
            feed_all(&["%begin 1 7 1", "%begin 1 8 1"])[1],
            Event::Desync(_)
        ));
    }

    #[test]
    fn exit_is_reported_and_a_truncated_reply_never_completes() {
        assert_eq!(feed_all(&["%exit"])[0], Event::Exit);
        let ev = feed_all(&["%begin 1 7 1", "partial"]);
        assert!(ev.iter().all(|e| *e == Event::Nothing));
    }

    #[test]
    fn carriage_returns_are_tolerated() {
        let ev = feed_all(&["%begin 1 7 1\r", "a\r", "%end 1 7 1\r"]);
        assert_eq!(ev[2], reply(&["a"], true));
    }
}
