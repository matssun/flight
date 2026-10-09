// SPDX-License-Identifier: MIT

/// Whether a provider's sessions can be continued by this node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Supported,
    /// No reliable way to learn or choose the session, so none is guessed. The reason is for
    /// the person who asks why a workspace was replaced instead of resumed.
    Unsupported(&'static str),
}

/// What is known about each provider. A provider not listed is unsupported.
pub fn resume_support(provider: &str) -> Support {
    match provider {
        // `claude --session-id <uuid>` starts a session with an identity we choose, and
        // `claude --resume <uuid>` continues it; both are documented and were checked against
        // the installed CLI.
        "claude" => Support::Supported,
        // Codex assigns its session id itself. It documents no way to choose one at launch and
        // none to learn the id of an interactive session short of its own picker; reading its
        // rollout files, or the terminal, would be guessing.
        "codex" => Support::Unsupported(
            "Codex chooses its session ids itself and does not report them to Flight",
        ),
        _ => Support::Unsupported("this provider has no session mechanism Flight knows"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_provider_with_a_documented_mechanism_is_supported() {
        assert_eq!(resume_support("claude"), Support::Supported);
        assert!(matches!(resume_support("codex"), Support::Unsupported(_)));
        assert!(matches!(
            resume_support("anything"),
            Support::Unsupported(_)
        ));
    }
}
