// SPDX-License-Identifier: MIT

/// The identity of an agent's session, as the node chose it before the agent started. Sensitive:
/// where the provider keeps the conversation on this machine, it is the key to it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AgentSession {
    pub provider: &'static str,
    pub token: String,
}

impl std::fmt::Debug for AgentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AgentSession({})", self.provider)
    }
}

/// How an agent is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentLaunch {
    /// A new session, with an identity of the node's choosing.
    New,
    /// The earlier session, already checked (see `Claude::check`).
    Resume(AgentSession),
}
