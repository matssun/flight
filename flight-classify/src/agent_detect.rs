// SPDX-License-Identifier: MIT

use crate::AgentKind;

/// Commands tmux reports for known agents with no manifest of their own.
const OTHER_AGENTS: [&str; 5] = ["aider", "cursor", "gemini", "amp", "droid"];

/// Which agent, if any, runs in a pane, from its foreground command.
///
/// v0 heuristic: `pane_current_command` only. Claude Code's executable is often named for its
/// version (`2.1.138`), so a bare semver-like command counts as Claude. Process-table
/// discovery (Fleet's approach) is a later adapter.
pub fn detect_agent(command: &str) -> Option<AgentKind> {
    match command {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "pi" => Some(AgentKind::Pi),
        "opencode" => Some(AgentKind::OpenCode),
        c if OTHER_AGENTS.contains(&c) => Some(AgentKind::Other),
        c if looks_like_version(c) => Some(AgentKind::Claude),
        _ => None,
    }
}

fn looks_like_version(c: &str) -> bool {
    let mut parts = c.split('.');
    let numeric = |p: Option<&str>| {
        p.is_some_and(|s| !s.is_empty() && s.chars().all(|ch| ch.is_ascii_digit()))
    };
    numeric(parts.next())
        && numeric(parts.next())
        && numeric(parts.next())
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_agents() {
        assert_eq!(detect_agent("claude"), Some(AgentKind::Claude));
        assert_eq!(detect_agent("codex"), Some(AgentKind::Codex));
        assert_eq!(detect_agent("aider"), Some(AgentKind::Other));
    }

    #[test]
    fn version_named_claude_binary() {
        assert_eq!(detect_agent("2.1.138"), Some(AgentKind::Claude));
        assert_eq!(detect_agent("2.1"), None);
        assert_eq!(detect_agent("2.1.x"), None);
        assert_eq!(detect_agent("1.2.3.4"), None);
    }

    #[test]
    fn shells_and_editors_are_not_agents() {
        for c in ["zsh", "bash", "vim", "node", "ssh", ""] {
            assert_eq!(detect_agent(c), None, "{c:?}");
        }
    }
}
