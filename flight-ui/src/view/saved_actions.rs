// SPDX-License-Identifier: MIT

//! What the dashboard does with a selected saved workspace: restore it, look again, ask before
//! it forgets, trusts or re-points one. Kept apart from the model's data.

use super::saved_prompt::failure_text;
use super::{
    Effect, SavedActionKind, SavedActionRequest, SavedOp, SavedPrompt, SavedPromptInput,
    SavedPromptKind, SavedPromptOutcome, ViewModel,
};
use crate::collect::CreateFailure;
use crate::snapshot::{SavedRoot, SavedView};

impl ViewModel {
    pub fn saved_prompt(&self) -> Option<&SavedPrompt> {
        self.saved_prompt.as_ref()
    }

    fn saved_request(saved: &SavedView, action: SavedActionKind) -> SavedActionRequest {
        SavedActionRequest {
            host: saved.host.clone(),
            host_label: saved.host_label.clone(),
            config_key: saved.config_key.clone(),
            name: saved.name.clone(),
            action,
        }
    }

    /// Enter on a saved workspace: start it again. Explicit and idempotent, so it asks nothing;
    /// the node refuses, with the reason, whatever is not safe to start.
    pub(super) fn restore_selected(&mut self) -> Option<Effect> {
        let saved = self.selected_saved()?;
        self.message = Some(format!("Starting {}…", saved.name));
        Some(Effect::SavedAction(Self::saved_request(
            &saved,
            SavedActionKind::Restore,
        )))
    }

    /// `r` on a saved workspace: look again now, instead of at the next periodic check.
    pub(super) fn retry_selected(&mut self) -> Option<Effect> {
        let saved = self.selected_saved()?;
        self.message = Some(format!("Checking {} again…", saved.name));
        Some(Effect::SavedAction(Self::saved_request(
            &saved,
            SavedActionKind::Retry,
        )))
    }

    pub(super) fn saved_op(&mut self, op: SavedOp) -> Effect {
        let Some(saved) = self.selected_saved() else {
            return Effect::None;
        };
        let kind = match op {
            SavedOp::ChangeRoot => {
                self.saved_prompt = Some(SavedPrompt::change_root(saved));
                return Effect::None;
            }
            SavedOp::Remove => SavedPromptKind::Remove,
            SavedOp::AcceptRoot if saved.root_state == SavedRoot::Changed => {
                SavedPromptKind::AcceptRoot
            }
            SavedOp::AcceptRoot => {
                self.message = Some(format!(
                    "{}: the directory has not changed, nothing to accept.",
                    saved.name
                ));
                return Effect::None;
            }
            SavedOp::Trust if saved.imported => SavedPromptKind::Trust,
            SavedOp::Trust => {
                self.message = Some(format!("{} was made here; nothing to trust.", saved.name));
                return Effect::None;
            }
        };
        self.saved_prompt = Some(SavedPrompt::new(saved, kind));
        Effect::None
    }

    pub(super) fn apply_saved_prompt(&mut self, input: SavedPromptInput) -> Effect {
        let Some(prompt) = self.saved_prompt.as_mut() else {
            return Effect::None;
        };
        match prompt.handle(input) {
            SavedPromptOutcome::None => Effect::None,
            SavedPromptOutcome::Cancel => {
                self.saved_prompt = None;
                Effect::None
            }
            SavedPromptOutcome::Submit(request) => Effect::SavedAction(request),
        }
    }

    /// The answer to an [`Effect::SavedAction`]. A prompt that asked stays open on a refusal;
    /// otherwise the outcome is a line at the bottom.
    pub fn apply_saved_action(
        &mut self,
        request: &SavedActionRequest,
        result: Result<(), CreateFailure>,
    ) {
        match result {
            Ok(()) => {
                self.saved_prompt = None;
                self.message = Some(done_text(request));
            }
            Err(failure) => match self.saved_prompt.as_mut() {
                Some(prompt) => prompt.fail(&failure),
                None => {
                    self.message = Some(format!(
                        "{}: {}",
                        request.name,
                        failure_text(&failure, &request.host_label)
                    ));
                }
            },
        }
    }
}

fn done_text(request: &SavedActionRequest) -> String {
    let name = &request.name;
    match &request.action {
        SavedActionKind::Retry => format!("Checked {name} again."),
        SavedActionKind::Remove => format!(
            "Forgot {name}. Nothing on {} was deleted.",
            request.host_label
        ),
        SavedActionKind::Restore => format!("Started {name} on {}.", request.host_label),
        SavedActionKind::AcceptRoot => format!("{name}: that directory is now the saved one."),
        SavedActionKind::SetRoot(path) => format!("{name} now points at {path}."),
        SavedActionKind::Trust => format!("{name} is trusted and may be started."),
    }
}
