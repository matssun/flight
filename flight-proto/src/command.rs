// SPDX-License-Identifier: MIT

use crate::command_kind::Kind;
use crate::validate::non_empty;
use crate::{valid_dir, valid_session_name};
use crate::{PaneRefMsg, ProgramCode, Reject, Validate};

/// The most preview lines a single request may ask for.
pub const MAX_PREVIEW_LINES: u32 = 2000;

/// A routed control request. Each variant names the pane (and so the host) it targets;
/// `CreateSession` names the host explicitly.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Command {
    #[prost(oneof = "command_kind::Kind", tags = "1, 3, 4, 6, 7, 8")]
    pub kind: Option<command_kind::Kind>,
}

pub mod command_kind {
    use crate::{PaneRefMsg, ProgramCode};

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

    /// Open an interactive terminal onto the pane: the node starts a tmux client in a PTY it
    /// owns, attached to this pane, guarded by `expected_pid` (ADR-003). The caller never
    /// chooses `terminal_id`; the orchestrator mints it and hands it to the node.
    #[derive(Clone, PartialEq, Eq, prost::Message)]
    pub struct OpenTerminal {
        #[prost(message, optional, tag = "1")]
        pub pane_ref: Option<PaneRefMsg>,
        #[prost(uint32, tag = "2")]
        pub expected_pid: u32,
        #[prost(uint32, tag = "3")]
        pub cols: u32,
        #[prost(uint32, tag = "4")]
        pub rows: u32,
        /// The `TERM` of the terminal the user is looking at.
        #[prost(string, tag = "5")]
        pub term: String,
        /// Empty from a UI (a UI-supplied id is rejected); 16 bytes toward a node.
        #[prost(bytes = "vec", tag = "6")]
        pub terminal_id: Vec<u8>,
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

    /// A detached session on the node's own tmux server. The program is a [`ProgramCode`],
    /// never a command line. Tag 5 of [`Kind`] was an earlier `CreateSession` that carried
    /// one; it is retired, not reused.
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
        #[prost(enumeration = "ProgramCode", tag = "5")]
        pub program: i32,
    }

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        GetPreview(GetPreview),
        #[prost(message, tag = "3")]
        SendInput(SendInput),
        #[prost(message, tag = "4")]
        KillPane(KillPane),
        #[prost(message, tag = "6")]
        RevealPane(RevealPane),
        #[prost(message, tag = "7")]
        OpenTerminal(OpenTerminal),
        #[prost(message, tag = "8")]
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
            RevealPane(c) => host_of(&c.pane_ref),
            OpenTerminal(c) => host_of(&c.pane_ref),
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
            OpenTerminal(c) => {
                pane_ref(&c.pane_ref)?;
                if c.expected_pid == 0 {
                    return Err(Reject::OutOfRange("open_terminal.expected_pid"));
                }
                if !crate::terminal::valid_dims(c.cols, c.rows) {
                    return Err(Reject::OutOfRange("open_terminal.size"));
                }
                if !crate::valid_term(&c.term) {
                    return Err(Reject::OutOfRange("open_terminal.term"));
                }
                if !c.terminal_id.is_empty() && c.terminal_id.len() != crate::TERMINAL_ID_LEN {
                    return Err(Reject::OutOfRange("open_terminal.terminal_id"));
                }
                Ok(())
            }
            SendInput(c) => pane_ref(&c.pane_ref),
            KillPane(c) => pane_ref(&c.pane_ref),
            CreateSession(c) => {
                non_empty(&c.host, "create_session.host")?;
                non_empty(&c.server, "create_session.server")?;
                if !valid_session_name(&c.name) {
                    return Err(Reject::OutOfRange("create_session.name"));
                }
                if !valid_dir(&c.dir) {
                    return Err(Reject::OutOfRange("create_session.dir"));
                }
                ProgramCode::decode(c.program, "create_session.program").map(|_| ())
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

impl Request {
    /// Validate a request that came from a UI: a UI never chooses a terminal id.
    pub fn validate_from_ui(&self) -> Result<(), Reject> {
        self.validate()?;
        if let Some(Kind::OpenTerminal(open)) = self.command.as_ref().and_then(|c| c.kind.as_ref())
        {
            if !open.terminal_id.is_empty() {
                return Err(Reject::Mismatch("open_terminal.terminal_id"));
            }
        }
        Ok(())
    }
}

impl Validate for Request {
    fn validate(&self) -> Result<(), Reject> {
        self.command
            .as_ref()
            .ok_or(Reject::Missing("request.command"))?
            .validate()
    }
}
