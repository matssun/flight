// SPDX-License-Identifier: MIT

use super::{Field, FormInput, HostChoice, NewSessionRequest, Program};
use crate::collect::CreateFailure;
use flight_state::{valid_dir, valid_session_name, HostId, MAX_SESSION_NAME_LEN};

/// Longest text a field accepts: the wire limits, so nothing typed is refused later for size.
const MAX_DIR_INPUT: usize = flight_state::MAX_DIR_LEN;

/// What the form wants done after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormOutcome {
    None,
    Cancel,
    Submit(NewSessionRequest),
}

/// The "New session" form: pure state, no I/O. Validation happens here, before anything is
/// sent; problems are shown in the form and never close it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSessionForm {
    hosts: Vec<HostChoice>,
    host: usize,
    name: String,
    dir: String,
    program: Program,
    focus: Field,
    error: Option<String>,
    submitting: bool,
}

impl NewSessionForm {
    /// A fresh form over the nodes currently connected, starting on `prefer` when it is one of
    /// them (the host of the pane the user was looking at).
    pub fn new(hosts: Vec<HostChoice>, prefer: Option<&HostId>) -> Self {
        let host = prefer
            .and_then(|p| hosts.iter().position(|h| &h.host == p))
            .unwrap_or(0);
        Self {
            hosts,
            host,
            name: String::new(),
            dir: "~".to_owned(),
            program: Program::Claude,
            focus: Field::Name,
            error: None,
            submitting: false,
        }
    }

    pub fn hosts(&self) -> &[HostChoice] {
        &self.hosts
    }

    pub fn host_index(&self) -> usize {
        self.host
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dir(&self) -> &str {
        &self.dir
    }

    pub fn program(&self) -> Program {
        self.program
    }

    pub fn focus(&self) -> Field {
        self.focus
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn submitting(&self) -> bool {
        self.submitting
    }

    pub fn handle(&mut self, input: FormInput) -> FormOutcome {
        if self.submitting {
            // The request is on its way; its answer decides what happens next.
            return FormOutcome::None;
        }
        match input {
            FormInput::Cancel => return FormOutcome::Cancel,
            FormInput::Next => self.focus = self.focus.next(),
            FormInput::Prev => self.focus = self.focus.prev(),
            FormInput::Left => self.change(false),
            FormInput::Right => self.change(true),
            FormInput::Char(c) => self.type_char(c),
            FormInput::Backspace => {
                if let Some(text) = self.text_mut() {
                    text.pop();
                }
                self.error = None;
            }
            FormInput::Enter => return self.activate(),
        }
        FormOutcome::None
    }

    /// The node refused (or could not be reached): say so in the form and put the focus on
    /// what to fix. The form stays open.
    pub fn fail(&mut self, failure: &CreateFailure) {
        self.submitting = false;
        let host = self
            .hosts
            .get(self.host)
            .map_or("the node", |h| h.label.as_str());
        let (focus, message) = match failure {
            CreateFailure::AlreadyExists => (
                Field::Name,
                format!(
                    "A session named \"{}\" already exists on {host}. Choose another name.",
                    self.name
                ),
            ),
            CreateFailure::NoSuchDirectory(why) => (Field::Directory, why.clone()),
            CreateFailure::ProgramUnavailable(why) => (Field::Start, why.clone()),
            CreateFailure::Unreachable => (
                Field::Host,
                format!("{host} is not connected right now. Nothing was created."),
            ),
            CreateFailure::Unsupported => (
                Field::Create,
                "This dashboard is not connected to an orchestrator, so it cannot create sessions."
                    .to_owned(),
            ),
            CreateFailure::Other(why) => (Field::Create, why.clone()),
        };
        self.focus = focus;
        self.error = Some(message);
    }

    fn activate(&mut self) -> FormOutcome {
        match self.focus {
            Field::Create => self.submit(),
            Field::Cancel => FormOutcome::Cancel,
            _ => {
                self.focus = self.focus.next();
                FormOutcome::None
            }
        }
    }

    fn submit(&mut self) -> FormOutcome {
        match self.validate() {
            Ok(request) => {
                self.error = None;
                self.submitting = true;
                FormOutcome::Submit(request)
            }
            Err((field, message)) => {
                self.focus = field;
                self.error = Some(message.to_owned());
                FormOutcome::None
            }
        }
    }

    fn validate(&self) -> Result<NewSessionRequest, (Field, &'static str)> {
        let host = self.hosts.get(self.host).ok_or((
            Field::Host,
            "No node is connected, so there is nowhere to create a session.",
        ))?;
        if self.name.is_empty() {
            return Err((Field::Name, "Enter a name for the session."));
        }
        if !valid_session_name(&self.name) {
            return Err((
                Field::Name,
                "Use only letters, digits, - and _ in the name (up to 64 characters).",
            ));
        }
        if self.dir.trim().is_empty() {
            return Err((Field::Directory, "Enter the directory to start in."));
        }
        if !valid_dir(&self.dir) {
            return Err((
                Field::Directory,
                "Use a full path starting with /, or start with ~ for the node's home.",
            ));
        }
        Ok(NewSessionRequest {
            host: host.host.clone(),
            host_label: host.label.clone(),
            name: self.name.clone(),
            dir: self.dir.clone(),
            program: self.program,
        })
    }

    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Host if !self.hosts.is_empty() => {
                let n = self.hosts.len();
                self.host = if forward {
                    self.host.saturating_add(1) % n
                } else {
                    self.host.checked_sub(1).unwrap_or(n.saturating_sub(1))
                };
                self.error = None;
            }
            Field::Start => {
                self.program = if forward {
                    self.program.next()
                } else {
                    self.program.prev()
                };
                self.error = None;
            }
            Field::Create | Field::Cancel => {
                self.focus = if forward {
                    Field::Cancel
                } else {
                    Field::Create
                };
            }
            _ => {}
        }
    }

    fn type_char(&mut self, c: char) {
        match self.focus {
            Field::Start if c == ' ' => self.program = self.program.next(),
            Field::Host if c == ' ' => self.change(true),
            _ => {
                let limit = if self.focus == Field::Name {
                    MAX_SESSION_NAME_LEN
                } else {
                    MAX_DIR_INPUT
                };
                if !c.is_control() {
                    if let Some(text) = self.text_mut() {
                        if text.len().saturating_add(c.len_utf8()) <= limit {
                            text.push(c);
                        }
                    }
                }
            }
        }
        self.error = None;
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            Field::Name => Some(&mut self.name),
            Field::Directory => Some(&mut self.dir),
            _ => None,
        }
    }
}
