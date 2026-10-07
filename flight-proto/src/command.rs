// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{PaneRefMsg, Reject, Validate};

/// The most preview lines a single request may ask for.
pub const MAX_PREVIEW_LINES: u32 = 2000;

/// A routed control request. Each variant names the pane (and so the host) it targets;
/// `CreateSession` names the host explicitly.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Command {
    #[prost(oneof = "command_kind::Kind", tags = "1, 3, 4, 5, 6")]
    pub kind: Option<command_kind::Kind>,
}

pub mod command_kind {
    use crate::PaneRefMsg;

    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct GetPreview {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
        #[prost(uint32, tag = "2")]
        pub lines: u32,
    }

    /// Make the pane the active pane of its window and its window the current window of its
    /// session, on the node's own tmux server. It never touches a tmux client: presenting the
    /// pane to a person is the caller's business (ADR-003). Tag 2 was an unguarded
    /// `SwitchPane` that no node ever offered; it is retired, not reused.
    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct RevealPane {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
        /// The pane process the caller was looking at. A pane id is reused across process
        /// lifetimes, so the node refuses unless this is still the pane's process.
        #[prost(uint32, tag = "2")]
        pub expected_pid: u32,
    }

    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct SendInput {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
        #[prost(string, tag = "2")]
        pub text: String,
        /// Press Enter after the text.
        #[prost(bool, tag = "3")]
        pub enter: bool,
    }

    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct KillPane {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
    }

    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct CreateSession {
        #[prost(string, tag = "1")]
        pub host: String,
        #[prost(string, tag = "2")]
        pub server: String,
        #[prost(string, tag = "3")]
        pub name: String,
        #[prost(string, tag = "4")]
        pub dir: String,
        /// Empty means the shell.
        #[prost(string, tag = "5")]
        pub command: String,
    }

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        GetPreview(GetPreview),
        #[prost(message, tag = "3")]
        SendInput(SendInput),
        #[prost(message, tag = "4")]
        KillPane(KillPane),
        #[prost(message, tag = "5")]
        CreateSession(CreateSession),
        #[prost(message, tag = "6")]
        RevealPane(RevealPane),
    }
}

fn host_of(p: &Option<PaneRefMsg>) -> Option<&str> {
    p.as_ref().map(|r| r.host.as_str())
}

impl Command {
    /// The host this command must be routed to, if the command is well-formed enough to say.
    pub fn target_host(&self) -> Option<&str> {
        use command_kind::Kind::*;
        match self.kind.as_ref()? {
            GetPreview(c) => host_of(&c.pane_ref),
            RevealPane(c) => host_of(&c.pane_ref),
            SendInput(c) => host_of(&c.pane_ref),
            KillPane(c) => host_of(&c.pane_ref),
            CreateSession(c) => Some(c.host.as_str()),
        }
    }
}

fn pane_ref(p: &Option<PaneRefMsg>) -> Result<(), Reject> {
    p.as_ref()
        .ok_or(Reject::Missing("command.pane_ref"))?
        .validate()
}

impl Validate for Command {
    fn validate(&self) -> Result<(), Reject> {
        use command_kind::Kind::*;
        match self.kind.as_ref().ok_or(Reject::Missing("command.kind"))? {
            GetPreview(c) => {
                pane_ref(&c.pane_ref)?;
                if c.lines == 0 || c.lines > MAX_PREVIEW_LINES {
                    return Err(Reject::OutOfRange("get_preview.lines"));
                }
                Ok(())
            }
            RevealPane(c) => {
                pane_ref(&c.pane_ref)?;
                if c.expected_pid == 0 {
                    return Err(Reject::OutOfRange("reveal_pane.expected_pid"));
                }
                Ok(())
            }
            SendInput(c) => pane_ref(&c.pane_ref),
            KillPane(c) => pane_ref(&c.pane_ref),
            CreateSession(c) => {
                non_empty(&c.host, "create_session.host")?;
                non_empty(&c.server, "create_session.server")?;
                non_empty(&c.name, "create_session.name")
            }
        }
    }
}

/// A command with the id its response will carry.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Request {
    #[prost(uint64, tag = "1")]
    pub request_id: u64,
    #[prost(message, optional, tag = "2")]
    pub command: Option<Command>,
}

impl Validate for Request {
    fn validate(&self) -> Result<(), Reject> {
        self.command
            .as_ref()
            .ok_or(Reject::Missing("request.command"))?
            .validate()
    }
}
