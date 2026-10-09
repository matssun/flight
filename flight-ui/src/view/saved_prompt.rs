// SPDX-License-Identifier: MIT

use super::{SavedActionKind, SavedActionRequest, SavedPromptInput};
use crate::collect::CreateFailure;
use crate::snapshot::SavedView;
use flight_state::valid_dir;

/// What the prompt asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedPromptKind {
    /// Forget the saved reference.
    Remove,
    /// The directory now at the path is the one meant.
    AcceptRoot,
    /// An imported workspace may start processes.
    Trust,
    /// Point at another directory; the text being typed.
    ChangeRoot(String),
}

/// Which button has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedPromptButton {
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedPromptOutcome {
    None,
    Cancel,
    Submit(SavedActionRequest),
}

/// A question about one saved workspace, answered before anything is sent. Pure state, like the
/// other prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedPrompt {
    saved: SavedView,
    kind: SavedPromptKind,
    focus: SavedPromptButton,
    error: Option<String>,
    submitting: bool,
}

impl SavedPrompt {
    pub fn new(saved: SavedView, kind: SavedPromptKind) -> Self {
        // The default answer is the safe one: nothing is forgotten or trusted by a stray Enter.
        let focus = match kind {
            SavedPromptKind::ChangeRoot(_) => SavedPromptButton::Confirm,
            _ => SavedPromptButton::Cancel,
        };
        Self {
            saved,
            kind,
            focus,
            error: None,
            submitting: false,
        }
    }

    pub fn change_root(saved: SavedView) -> Self {
        let current = saved.root.clone();
        Self::new(saved, SavedPromptKind::ChangeRoot(current))
    }

    pub fn saved(&self) -> &SavedView {
        &self.saved
    }

    pub fn kind(&self) -> &SavedPromptKind {
        &self.kind
    }

    pub fn focus(&self) -> SavedPromptButton {
        self.focus
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn submitting(&self) -> bool {
        self.submitting
    }

    /// Whether keys type into a field (the directory) rather than answer yes or no.
    pub fn typing(&self) -> bool {
        matches!(self.kind, SavedPromptKind::ChangeRoot(_))
    }

    pub fn handle(&mut self, input: SavedPromptInput) -> SavedPromptOutcome {
        if self.submitting {
            return SavedPromptOutcome::None;
        }
        match (input, &mut self.kind) {
            (SavedPromptInput::Cancel, _) => SavedPromptOutcome::Cancel,
            (SavedPromptInput::Char(c), SavedPromptKind::ChangeRoot(text)) => {
                text.push(c);
                self.error = None;
                SavedPromptOutcome::None
            }
            (SavedPromptInput::Backspace, SavedPromptKind::ChangeRoot(text)) => {
                text.pop();
                self.error = None;
                SavedPromptOutcome::None
            }
            (SavedPromptInput::Char(_) | SavedPromptInput::Backspace, _) => {
                SavedPromptOutcome::None
            }
            (SavedPromptInput::Next, _) => {
                self.focus = match self.focus {
                    SavedPromptButton::Confirm => SavedPromptButton::Cancel,
                    SavedPromptButton::Cancel => SavedPromptButton::Confirm,
                };
                SavedPromptOutcome::None
            }
            (SavedPromptInput::Yes, _) => self.submit(),
            (SavedPromptInput::Enter, _) => match self.focus {
                SavedPromptButton::Confirm => self.submit(),
                SavedPromptButton::Cancel => SavedPromptOutcome::Cancel,
            },
        }
    }

    fn submit(&mut self) -> SavedPromptOutcome {
        let action = match &self.kind {
            SavedPromptKind::Remove => SavedActionKind::Remove,
            SavedPromptKind::AcceptRoot => SavedActionKind::AcceptRoot,
            SavedPromptKind::Trust => SavedActionKind::Trust,
            SavedPromptKind::ChangeRoot(text) => {
                let text = text.trim().to_owned();
                if !valid_dir(&text) {
                    self.error =
                        Some("A directory is an absolute path, or ~/ followed by one.".to_owned());
                    return SavedPromptOutcome::None;
                }
                SavedActionKind::SetRoot(text)
            }
        };
        self.error = None;
        self.submitting = true;
        SavedPromptOutcome::Submit(SavedActionRequest {
            host: self.saved.host.clone(),
            host_label: self.saved.host_label.clone(),
            config_key: self.saved.config_key.clone(),
            name: self.saved.name.clone(),
            action,
        })
    }

    /// The node refused (or could not be reached): say so, and stay open.
    pub fn fail(&mut self, failure: &CreateFailure) {
        self.submitting = false;
        self.error = Some(failure_text(failure, &self.saved.host_label));
    }
}

/// A refusal in words the user can act on.
pub fn failure_text(failure: &CreateFailure, host: &str) -> String {
    match failure {
        CreateFailure::Unreachable => {
            format!("{host} is not connected right now. Nothing was changed.")
        }
        CreateFailure::UnknownWorkspace => {
            "That saved workspace is gone from its host. Nothing was changed.".to_owned()
        }
        CreateFailure::Unsupported => {
            "This dashboard is not connected to an orchestrator.".to_owned()
        }
        CreateFailure::NoSuchDirectory(why)
        | CreateFailure::ProgramUnavailable(why)
        | CreateFailure::Other(why) => why.clone(),
        CreateFailure::AlreadyExists => {
            "More than one running workspace could be this one; nothing was started.".to_owned()
        }
    }
}
