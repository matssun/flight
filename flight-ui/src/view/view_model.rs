// SPDX-License-Identifier: MIT

use super::lists::section_panes;
use super::{Action, Effect, Section};
use crate::snapshot::{PanePreview, UiSnapshot};
use flight_state::PaneRef;

/// Presentation state: the latest snapshot plus what the user is pointing at. Selection is
/// the identity of a pane, never a row number, so reordering cannot move the cursor onto a
/// different agent; the visible row is derived from it each frame.
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub(super) snapshot: UiSnapshot,
    pub(super) selected: Option<PaneRef>,
    pub(super) focus: Section,
    /// Index the selection had in the focused list, to pick a neighbour if it vanishes.
    pub(super) hint: usize,
    pub(super) preview: Option<PanePreview>,
    pub(super) message: Option<String>,
    pub(super) loaded: bool,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewModel {
    pub fn new() -> Self {
        Self {
            snapshot: UiSnapshot::default(),
            selected: None,
            focus: Section::Attention,
            hint: 0,
            preview: None,
            message: None,
            loaded: false,
        }
    }

    pub fn snapshot(&self) -> &UiSnapshot {
        &self.snapshot
    }

    pub fn selected(&self) -> Option<&PaneRef> {
        self.selected.as_ref()
    }

    pub fn focus(&self) -> Section {
        self.focus
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// The preview, only if it is for the currently selected pane (never a stale one).
    pub fn preview(&self) -> Option<&PanePreview> {
        self.preview
            .as_ref()
            .filter(|p| Some(&p.pane) == self.selected.as_ref())
    }

    pub fn set_message(&mut self, m: Option<String>) {
        self.message = m;
    }

    pub fn apply_preview(&mut self, p: Option<PanePreview>) {
        self.preview = p;
    }

    /// Take a new snapshot and keep the cursor on the same pane where it still exists.
    pub fn apply_snapshot(&mut self, s: UiSnapshot) -> Effect {
        self.snapshot = s;
        self.loaded = true;
        let before = self.selected.clone();
        self.reconcile();
        self.select_effect(before)
    }

    pub fn apply(&mut self, action: Action) -> Effect {
        match action {
            Action::Quit => Effect::Quit,
            Action::Refresh => Effect::Refresh,
            Action::Up => self.step(-1),
            Action::Down => self.step(1),
            Action::ToggleFocus => self.toggle_focus(),
            Action::Switch => self.selected.clone().map_or(Effect::None, Effect::Switch),
        }
    }

    pub(super) fn select_effect(&self, before: Option<PaneRef>) -> Effect {
        if before == self.selected {
            Effect::None
        } else {
            Effect::Select(self.selected.clone())
        }
    }

    pub(super) fn refs(&self, section: Section) -> Vec<PaneRef> {
        section_panes(&self.snapshot, section)
            .into_iter()
            .map(|p| p.pane_ref.clone())
            .collect()
    }
}
