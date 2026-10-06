// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{PaneRefMsg, Reject, Validate};

/// The most preview lines a single request may ask for.
pub const MAX_PREVIEW_LINES: u32 = 2000;

/// A routed control request. Each variant names the pane (and so the host) it targets;
/// `CreateSession` names the host explicitly.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Command {
    #[prost(oneof = "command_kind::Kind", tags = "1, 2, 3, 4, 5")]
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

    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct SwitchPane {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
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
        #[prost(message, tag = "2")]
        SwitchPane(SwitchPane),
        #[prost(message, tag = "3")]
        SendInput(SendInput),
        #[prost(message, tag = "4")]
        KillPane(KillPane),
        #[prost(message, tag = "5")]
        CreateSession(CreateSession),
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
            SwitchPane(c) => host_of(&c.pane_ref),
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
            SwitchPane(c) => pane_ref(&c.pane_ref),
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
