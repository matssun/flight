// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

/// The longest token a provider may hand over.
pub const MAX_TOKEN_LEN: usize = 128;

/// Where a resume reference is good: the host, the workspace root (as the node canonicalized
/// it) and the operating-system user the agent ran as. A reference is never used outside the
/// scope it was made in: a provider keeps its conversations per user and per directory, so the
/// same token elsewhere means nothing, or someone else's conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeScope {
    pub host: String,
    pub root: String,
    pub user: String,
}

/// An opaque reference to an agent's earlier session, as a provider defines it. Flight does not
/// look inside it: the provider adapter made it (never read from terminal text) and is the only
/// thing that interprets it.
///
/// It is sensitive. Where the provider keeps the conversation on the same machine, the token
/// is the key to that history, so it is kept in a file of its own that only its owner can read,
/// is never part of an exported definition, and is never printed (`Debug` and `Display` show a
/// short tail at most).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeRef {
    /// The provider that made it (`claude`).
    pub provider: String,
    token: String,
    pub scope: ResumeScope,
}

impl ResumeRef {
    /// A reference, if the token is plain text of a sane length (it is placed on a command
    /// line, so it must be unable to carry anything else).
    pub fn new(provider: &str, token: &str, scope: ResumeScope) -> Option<Self> {
        let ok = !token.is_empty()
            && token.len() <= MAX_TOKEN_LEN
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            && !token.starts_with('-')
            && !provider.is_empty()
            && provider.len() <= 32
            && provider
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-');
        ok.then(|| Self {
            provider: provider.to_owned(),
            token: token.to_owned(),
            scope,
        })
    }

    /// The token, for the provider adapter that made it and nothing else.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Whether this reference was made for this scope.
    pub fn is_for(&self, scope: &ResumeScope) -> bool {
        self.scope == *scope
    }
}

impl std::fmt::Debug for ResumeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ResumeRef({}:…{})", self.provider, tail(&self.token))
    }
}

impl std::fmt::Display for ResumeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} session …{}", self.provider, tail(&self.token))
    }
}

fn tail(token: &str) -> &str {
    let start = token.len().saturating_sub(4);
    token.get(start..).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> ResumeScope {
        ResumeScope {
            host: "h".into(),
            root: "/work/a".into(),
            user: "501".into(),
        }
    }

    #[test]
    fn a_token_is_plain_text_that_cannot_alter_a_command_line() {
        assert!(
            ResumeRef::new("claude", "550e8400-e29b-41d4-a716-446655440000", scope()).is_some()
        );
        for bad in [
            "",
            "-x",
            "a b",
            "a;b",
            "$(x)",
            "a\nb",
            &"a".repeat(MAX_TOKEN_LEN + 1),
        ] {
            assert!(ResumeRef::new("claude", bad, scope()).is_none(), "{bad:?}");
        }
        assert!(ResumeRef::new("", "abc", scope()).is_none());
        assert!(ResumeRef::new("Claude Code", "abc", scope()).is_none());
    }

    #[test]
    fn the_token_never_appears_in_debug_or_display() {
        let r = ResumeRef::new("claude", "550e8400-e29b-41d4-a716-446655440000", scope()).unwrap();
        for text in [format!("{r:?}"), format!("{r}")] {
            assert!(!text.contains("550e8400"), "{text}");
            assert!(text.contains("0000"), "{text}");
        }
    }

    #[test]
    fn a_reference_is_good_only_in_the_scope_it_was_made_in() {
        let r = ResumeRef::new("claude", "abc", scope()).unwrap();
        assert!(r.is_for(&scope()));
        for other in [
            ResumeScope {
                host: "other".into(),
                ..scope()
            },
            ResumeScope {
                root: "/work/b".into(),
                ..scope()
            },
            ResumeScope {
                user: "0".into(),
                ..scope()
            },
        ] {
            assert!(!r.is_for(&other));
        }
    }
}
