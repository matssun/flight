// SPDX-License-Identifier: MIT

use super::in_use::session_in_use;
use super::{transcript, AgentSession};
use flight_workspaces::{ResumeRef, ResumeScope};
use std::path::{Path, PathBuf};

/// The provider name used in saved references.
pub(crate) const PROVIDER: &str = "claude";

/// Claude Code's session mechanism: the node chooses a session id (`--session-id`), and later
/// continues it (`--resume`). Both flags are documented; a conversation is kept as a transcript
/// per project directory; the CLI itself refuses an id that is already in use and says so for
/// one it does not know (checked against the installed CLI).
pub(crate) struct Claude;

impl Claude {
    /// A new session identity: a random (version 4) UUID, which is the form the flag accepts.
    pub(crate) fn new_session() -> Result<AgentSession, std::io::Error> {
        let mut b = [0u8; 16];
        getrandom::getrandom(&mut b).map_err(|_| std::io::Error::other("no randomness"))?;
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let mut token = String::with_capacity(36);
        for (i, byte) in b.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                token.push('-');
            }
            token.push_str(&format!("{byte:02x}"));
        }
        Ok(AgentSession {
            provider: PROVIDER,
            token,
        })
    }

    /// The command line of a new agent with a session identity of our choosing.
    pub(crate) fn new_argv(claude: PathBuf, session: &AgentSession, skip: bool) -> Vec<String> {
        let mut argv = vec![
            claude.to_string_lossy().into_owned(),
            "--session-id".to_owned(),
            session.token.clone(),
        ];
        if skip {
            argv.push("--dangerously-skip-permissions".to_owned());
        }
        argv
    }

    /// The command line that continues a session. It never carries over a permission mode: the
    /// provider restores some modes of an earlier run and not others, so the mode is set here,
    /// explicitly, to the one that asks before acting, unless the saved definition says the agent
    /// runs without asking (and recovery has been allowed to start such an agent).
    pub(crate) fn resume_argv(claude: PathBuf, session: &AgentSession, skip: bool) -> Vec<String> {
        let mut argv = vec![
            claude.to_string_lossy().into_owned(),
            "--resume".to_owned(),
            session.token.clone(),
        ];
        if skip {
            argv.push("--dangerously-skip-permissions".to_owned());
        } else {
            argv.extend(["--permission-mode".to_owned(), "default".to_owned()]);
        }
        argv
    }

    /// Whether `reference` may be used now, in `scope`: it was made for claude, for this host,
    /// directory and user; the provider still has the conversation for that directory; and no
    /// process is running that session. `Err` says why not, in words for the person.
    pub(crate) fn check(
        reference: &ResumeRef,
        scope: &ResumeScope,
        config_dir: Option<&Path>,
    ) -> Result<AgentSession, String> {
        if reference.provider != PROVIDER {
            return Err(format!("this reference is for {}", reference.provider));
        }
        if !reference.is_for(scope) {
            return Err(
                "the saved session was made for another host, directory or user".to_owned(),
            );
        }
        let config =
            config_dir.ok_or_else(|| "this node has no Claude data directory".to_owned())?;
        transcript::find(config, &scope.root, reference.token())?;
        if session_in_use(reference.token()) {
            return Err("that session is already running; reconnect to it instead".to_owned());
        }
        Ok(AgentSession {
            provider: PROVIDER,
            token: reference.token().to_owned(),
        })
    }
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
    fn a_new_session_is_a_v4_uuid_and_different_each_time() {
        let a = Claude::new_session().unwrap();
        let b = Claude::new_session().unwrap();
        assert_ne!(a.token, b.token);
        let parts: Vec<&str> = a.token.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(parts[3].starts_with(['8', '9', 'a', 'b']));
    }

    #[test]
    fn a_new_agent_gets_its_identity_and_a_resumed_one_asks_before_acting() {
        let s = AgentSession {
            provider: PROVIDER,
            token: "t".into(),
        };
        assert_eq!(
            Claude::new_argv("/bin/claude".into(), &s, false),
            ["/bin/claude", "--session-id", "t"]
        );
        assert_eq!(
            Claude::resume_argv("/bin/claude".into(), &s, false),
            [
                "/bin/claude",
                "--resume",
                "t",
                "--permission-mode",
                "default"
            ]
        );
        // Only the saved definition's own word brings the flag back.
        assert!(Claude::resume_argv("/bin/claude".into(), &s, true)
            .contains(&"--dangerously-skip-permissions".to_owned()));
        assert!(
            !Claude::resume_argv("/bin/claude".into(), &s, true).contains(&"default".to_owned())
        );
    }

    #[test]
    fn a_reference_for_another_provider_or_scope_is_refused_before_anything_is_looked_at() {
        let codex = ResumeRef::new("codex", "abc", scope()).unwrap();
        assert!(Claude::check(&codex, &scope(), None)
            .unwrap_err()
            .contains("codex"));
        let mine = ResumeRef::new("claude", "abc", scope()).unwrap();
        let elsewhere = ResumeScope {
            user: "0".into(),
            ..scope()
        };
        assert!(Claude::check(&mine, &elsewhere, None)
            .unwrap_err()
            .contains("another"));
    }

    #[test]
    fn a_missing_conversation_is_reported_not_replaced() {
        let base = std::env::temp_dir().join(format!("flight-claude-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let r = ResumeRef::new("claude", "11111111-2222-4333-8444-555555555555", scope()).unwrap();
        let why = Claude::check(&r, &scope(), Some(&base)).unwrap_err();
        assert!(why.contains("no saved conversation"), "{why}");
        let dir = base.join("projects").join("-work-a");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("11111111-2222-4333-8444-555555555555.jsonl"),
            "{}\n",
        )
        .unwrap();
        assert!(Claude::check(&r, &scope(), Some(&base)).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }
}
