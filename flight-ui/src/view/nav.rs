// SPDX-License-Identifier: MIT

//! Selection movement, search and reconciliation, kept apart from the model's data.

use super::{Effect, FilterInput, ViewModel};
use flight_state::PaneRef;

fn position(list: &[PaneRef], sel: Option<&PaneRef>) -> Option<usize> {
    sel.and_then(|s| list.iter().position(|p| p == s))
}

impl ViewModel {
    /// Make the selection valid for the new snapshot or filter without moving it onto another
    /// session unless its pane is gone from the list.
    pub(super) fn reconcile(&mut self) {
        let list = self.refs();
        if let Some(i) = position(&list, self.selected.as_ref()) {
            self.hint = i;
            return;
        }
        let idx = self.hint.min(list.len().saturating_sub(1));
        self.selected = list.get(idx).cloned();
        self.hint = idx;
    }

    /// Select the pane of a session just created, as soon as a snapshot has it. The wait ends
    /// when it shows up, when the user moves the cursor, or after a while.
    pub(super) fn select_created(&mut self) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        let found = self
            .snapshot
            .hosts
            .iter()
            .filter(|h| h.host == pending.host)
            .flat_map(|h| h.panes.iter())
            .find(|p| p.session == pending.session)
            .map(|p| p.pane_ref.clone());
        if let Some(pane) = found {
            // The new session must be visible: a search that hides it is dropped.
            if !self.refs().contains(&pane) {
                self.filter.clear();
                self.searching = false;
            }
            self.selected = Some(pane);
            self.hint = self
                .refs()
                .iter()
                .position(|p| Some(p) == self.selected.as_ref())
                .unwrap_or(0);
            self.pending = None;
        } else {
            pending.snapshots_left = pending.snapshots_left.saturating_sub(1);
            if pending.snapshots_left == 0 {
                self.pending = None;
            }
        }
    }

    pub(super) fn step(&mut self, delta: isize) -> Effect {
        self.pending = None;
        let before = self.selected.clone();
        let list = self.refs();
        let next = match position(&list, self.selected.as_ref()) {
            Some(i) => i
                .saturating_add_signed(delta)
                .min(list.len().saturating_sub(1)),
            None => 0,
        };
        self.selected = list.get(next).cloned();
        self.hint = next;
        self.select_effect(before)
    }

    /// Point at a listed session (a click). Anything not listed is ignored.
    pub(super) fn select(&mut self, pane: PaneRef) -> Effect {
        self.pending = None;
        let list = self.refs();
        let Some(i) = position(&list, Some(&pane)) else {
            return Effect::None;
        };
        let before = self.selected.clone();
        self.selected = Some(pane);
        self.hint = i;
        self.select_effect(before)
    }

    pub(super) fn filter_input(&mut self, input: FilterInput) -> Effect {
        match input {
            FilterInput::Char(c) => self.filter.push(c),
            FilterInput::Backspace => {
                self.filter.pop();
            }
            FilterInput::Accept => {
                self.searching = false;
                return Effect::None;
            }
            FilterInput::Clear => {
                self.filter.clear();
                self.searching = false;
            }
        }
        let before = self.selected.clone();
        self.reconcile();
        self.select_effect(before)
    }
}
