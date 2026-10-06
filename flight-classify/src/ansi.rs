// SPDX-License-Identifier: MIT

use regex::Regex;
use std::sync::OnceLock;

/// CSI sequences (incl. private modes like `ESC[?25l`) and OSC strings terminated by BEL
/// or ST; an unterminated OSC strips to end of input. Matches Fleet's `stripAnsi`.
const ANSI_PATTERN: &str = r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)?)";

pub(crate) fn strip_ansi(value: &str) -> String {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    match RE.get_or_init(|| Regex::new(ANSI_PATTERN).ok()) {
        Some(re) => re.replace_all(value, "").into_owned(),
        None => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn strips_csi_and_osc() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m \x1b[?25l"), "red ");
        assert_eq!(strip_ansi("a\x1b]8;;http://x\x07link\x1b]8;;\x07"), "alink");
        assert_eq!(strip_ansi("a\x1b]2;title"), "a");
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(strip_ansi("hello ❯"), "hello ❯");
    }
}
