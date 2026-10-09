// SPDX-License-Identifier: MIT

string_id! {
    /// The identity of a Flight workspace: the user's project/work context on one host. Minted
    /// by the node when the workspace is created and kept by the backend; a display name never
    /// stands in for it, and it is not a tmux id.
    WorkspaceId
}

/// The longest workspace or surface id the wire carries.
pub const MAX_ID_LEN: usize = 64;

/// An id as it travels: 1 to [`MAX_ID_LEN`] bytes of `[A-Za-z0-9_.-]`. Ids are placed in
/// backend metadata and in logs, so nothing else is accepted.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

impl WorkspaceId {
    /// The workspace of a session that predates workspaces, which carries no id of its own.
    /// It is named by the host and by the backend's own session identity (`$3`), not by the
    /// display name, so renaming the session does not change it. It lasts as long as that
    /// backend does. The host is part of it because backend ids repeat across machines.
    pub fn for_unmarked_session(host: &str, server: &str, session_id: &str) -> Self {
        let plain = |s: &str, max: usize| -> String {
            s.chars()
                .filter(|c| *c != '$')
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .take(max)
                .collect()
        };
        Self::new(format!(
            "legacy.{}.{}.{}",
            plain(host, 8),
            plain(server, 24),
            plain(session_id, 12)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_bounded_plain_text() {
        for ok in ["w-1a2b", "legacy.work.3", &"a".repeat(MAX_ID_LEN)] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "a b", "a:b", "$3", "a\n", &"a".repeat(MAX_ID_LEN + 1)] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn an_unmarked_session_gets_an_id_from_its_backend_identity() {
        let id = WorkspaceId::for_unmarked_session("ab12cd34ef", "work", "$3");
        assert!(valid_id(id.as_str()));
        assert_eq!(id.as_str(), "legacy.ab12cd34.work.3");
        assert_ne!(
            id,
            WorkspaceId::for_unmarked_session("ab12cd34ef", "work", "$4")
        );
        // The same backend ids on another machine are another workspace.
        assert_ne!(
            id,
            WorkspaceId::for_unmarked_session("99999999ff", "work", "$3")
        );
    }
}
