// SPDX-License-Identifier: MIT

//! Ported from the glyph check in Fleet's `discoverAgents` (src/agents/discovery.ts, MIT,
//! (c) 2026 Nick Nisi; see THIRD_PARTY.md).

use crate::ansi::strip_ansi;
use crate::builtin::WORKING_GLYPH;
use regex::Regex;
use std::sync::OnceLock;

/// Lines from the bottom the glyph check covers, so a spinner high in scrollback cannot read
/// as working. Matches the manifests' default rule window.
const GLYPH_WINDOW: usize = 15;

/// Is the braille working glyph (U+2800-U+28FF) on screen in the bottom window?
pub fn working_glyph_present(screen_lines: &[String]) -> bool {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    let Some(re) = RE.get_or_init(|| Regex::new(WORKING_GLYPH).ok()) else {
        return false;
    };
    let skip = screen_lines.len().saturating_sub(GLYPH_WINDOW);
    let window: Vec<&str> = screen_lines.iter().skip(skip).map(String::as_str).collect();
    re.is_match(&strip_ansi(&window.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::working_glyph_present;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn finds_the_glyph_in_the_bottom_window_only() {
        assert!(working_glyph_present(&lines(&["⠹ Puzzling…", "❯"])));
        assert!(!working_glyph_present(&lines(&["✶ Thinking…", "❯"])));
        let mut deep = vec!["⠹ old"];
        deep.extend(std::iter::repeat_n("scrollback", 20));
        assert!(!working_glyph_present(&lines(&deep)));
    }
}
