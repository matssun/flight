// SPDX-License-Identifier: MIT

//! Opening a workspace's surfaces: Enter, `a` and `s`; the offer to make a shell; and waiting
//! for a surface to be published before opening it. Kept apart from the model's data.

use super::lists::workspaces;
use super::view_model::PENDING_SNAPSHOTS;
use super::{Effect, NewSurfaceRequest, PromptOutcome, ShellPrompt, SurfaceChoice, ViewModel};
use crate::collect::CreateFailure;
use crate::snapshot::WorkspaceKey;

#[derive(Debug, Clone)]
pub(super) struct Opening {
    pub(super) key: WorkspaceKey,
    pub(super) choice: SurfaceChoice,
    /// The workspace is there but lacks the surface: offer to make a shell (true, for a
    /// terminal that was left to switch) or keep waiting for it to be published (false, for one
    /// just created).
    pub(super) offer_if_missing: bool,
    pub(super) snapshots_left: u8,
}

impl ViewModel {
    /// Open a surface of the selected workspace. The agent is the default (Enter); a workspace
    /// whose agent is gone opens its shell. A shell that does not exist yet is offered, not
    /// silently made.
    pub(super) fn open(&mut self, choice: SurfaceChoice) -> Effect {
        self.pending = None;
        if let Some(saved) = self.selected_saved() {
            self.message = Some(format!(
                "{} is not running; there is nothing to open. Enter starts it.",
                saved.name
            ));
            return Effect::None;
        }
        let Some(workspace) = self.selected_workspace() else {
            return Effect::None;
        };
        let surface = match choice {
            SurfaceChoice::Agent => workspace.agent().or_else(|| workspace.shell()),
            SurfaceChoice::Shell => workspace.shell(),
        };
        if let Some(surface) = surface {
            return Effect::Switch(surface.pane.clone());
        }
        match choice {
            SurfaceChoice::Shell => {
                self.prompt = Some(ShellPrompt::new(&workspace));
            }
            SurfaceChoice::Agent => {
                self.message = Some(format!("{} has nothing to open.", workspace.name));
            }
        }
        Effect::None
    }

    /// The user is asked to confirm a new shell; the answer decides what happens next.
    pub(super) fn apply_prompt(&mut self, input: super::PromptInput) -> Effect {
        let Some(prompt) = self.prompt.as_mut() else {
            return Effect::None;
        };
        match prompt.handle(input) {
            PromptOutcome::None => Effect::None,
            PromptOutcome::Cancel => {
                self.prompt = None;
                Effect::None
            }
            PromptOutcome::Submit(request) => Effect::CreateSurface(request),
        }
    }

    /// The answer to an [`Effect::CreateSurface`]. Success closes the prompt and opens the new
    /// surface as soon as the workspace publishes it; a refusal stays in the prompt.
    pub fn apply_surface_created(
        &mut self,
        request: &NewSurfaceRequest,
        result: Result<(), CreateFailure>,
    ) -> Effect {
        // The shell being there already is what the user wanted, however it came to be.
        let result = match result {
            Err(CreateFailure::AlreadyExists) => Ok(()),
            other => other,
        };
        match result {
            Ok(()) => {
                self.prompt = None;
                self.message = Some(format!(
                    "Created a {} for {}.",
                    request.kind.label(),
                    request.name
                ));
                self.opening = Some(Opening {
                    key: WorkspaceKey {
                        host: request.host.clone(),
                        workspace: request.workspace.clone(),
                    },
                    choice: request.kind,
                    offer_if_missing: false,
                    snapshots_left: PENDING_SNAPSHOTS,
                });
                self.open_pending()
            }
            Err(failure) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.fail(&failure);
                }
                Effect::None
            }
        }
    }

    /// Start with the cursor on this workspace (the one just left), if it is listed once the
    /// first snapshot arrives.
    pub fn point_at(&mut self, key: WorkspaceKey) {
        self.pointing = Some((key, PENDING_SNAPSHOTS));
    }

    pub(super) fn apply_pointing(&mut self) {
        let Some((key, left)) = self.pointing.take() else {
            return;
        };
        if let Some(i) = self.keys().iter().position(|k| *k == key) {
            self.hint = i;
            self.selected = Some(key);
        } else if left > 1 {
            self.pointing = Some((key, left.saturating_sub(1)));
        }
    }

    /// Whether a surface is being waited for: the loop then looks for news more often than
    /// its refresh interval, so opening it is not held back by the next poll.
    pub fn is_opening(&self) -> bool {
        self.opening.is_some()
    }

    /// Ask for a surface to be opened as soon as it exists (a terminal was left in order to
    /// switch to it).
    pub fn resume(&mut self, key: WorkspaceKey, choice: SurfaceChoice) {
        self.opening = Some(Opening {
            key,
            choice,
            offer_if_missing: true,
            snapshots_left: PENDING_SNAPSHOTS,
        });
    }

    /// Open the surface being waited for, if a snapshot has it now.
    pub(super) fn open_pending(&mut self) -> Effect {
        let Some(opening) = self.opening.as_mut() else {
            return Effect::None;
        };
        let found = self
            .snapshot
            .hosts
            .iter()
            .filter(|h| h.host == opening.key.host)
            .flat_map(|h| h.panes.iter())
            .filter(|p| p.workspace == opening.key.workspace)
            .find(|p| opening.choice.is(p.kind))
            .cloned();
        if let Some(pane) = found {
            self.opening = None;
            return Effect::Switch(pane);
        }
        // A workspace that is listed but lacks the surface is the answer, not a wait: a missing
        // shell is offered, a missing agent is left alone.
        let (key, choice, offer) = (
            opening.key.clone(),
            opening.choice,
            opening.offer_if_missing,
        );
        if let Some(workspace) = workspaces(&self.snapshot, "")
            .into_iter()
            .find(|w| w.key() == key && offer)
        {
            self.opening = None;
            if choice == SurfaceChoice::Shell {
                self.filter.clear();
                self.searching = false;
                self.selected = Some(key);
                self.prompt = Some(ShellPrompt::new(&workspace));
            }
            return Effect::None;
        }
        let Some(opening) = self.opening.as_mut() else {
            return Effect::None;
        };
        opening.snapshots_left = opening.snapshots_left.saturating_sub(1);
        if opening.snapshots_left == 0 {
            self.opening = None;
        }
        Effect::None
    }
}
