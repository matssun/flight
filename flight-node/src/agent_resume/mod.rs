// SPDX-License-Identifier: MIT

//! Continuing an agent's earlier session (ADR-010).
//!
//! What a node knows about a session is what it arranged: it chooses the session's identity
//! when it starts the agent (so nothing is ever read back out of terminal text), keeps that as
//! an opaque reference in a private file, and uses it only for the provider that made it, in the
//! user and directory it was made in, after checking that the provider still has the
//! conversation. A provider that cannot be started with an identity of the node's choosing, or
//! that gives no way to find its sessions, is unsupported, and says so.

mod agent_session;
mod claude;
mod in_use;
mod support;
mod transcript;

pub(crate) use agent_session::{AgentLaunch, AgentSession};
pub(crate) use claude::Claude;
pub(crate) use support::{resume_support, Support};
pub(crate) use transcript::config_dir;
