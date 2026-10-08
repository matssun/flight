// SPDX-License-Identifier: MIT

//! Selection movement and reconciliation, kept apart from the model's data.

use super::{Effect, Section, ViewModel};
use flight_state::PaneRef;

fn position(list: &[PaneRef], sel: Option<&PaneRef>) -> Option<usize> {
    sel.and_then(|s| list.iter().position(|p| p == s))
}

impl ViewModel {
    /// Make the selection valid for the new snapshot without moving it onto another agent
    /// unless its pane is gone.
    pub(super) fn reconcile(&mut self) {
        let in_focus = self.refs(self.focus);
        if let Some(i) = position(&in_focus, self.selected.as_ref()) {
            self.hint = i;
            return;
        }
        // The pane left this list but may still exist (e.g. it no longer needs attention):
        // follow it to the section that has it instead of jumping to a neighbour.
        let other = self.focus.other();
        let other_list = self.refs(other);
        if let Some(i) = position(&other_list, self.selected.as_ref()) {
            self.focus = other;
            self.hint = i;
            return;
        }
        // Gone entirely: take the neighbour at the same position, else fall back a section.
        self.pick_neighbour(in_focus, other_list);
    }

    fn pick_neighbour(&mut self, in_focus: Vec<PaneRef>, other_list: Vec<PaneRef>) {
        let (list, section) = if in_focus.is_empty() {
            (other_list, self.focus.other())
        } else {
            (in_focus, self.focus)
        };
        let idx = self.hint.min(list.len().saturating_sub(1));
        self.selected = list.get(idx).cloned();
        self.focus = section;
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
            self.selected = Some(pane);
            self.focus = Section::Tree;
            self.hint = self
                .refs(Section::Tree)
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
        let list = self.refs(self.focus);
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

    pub(super) fn toggle_focus(&mut self) -> Effect {
        self.pending = None;
        let before = self.selected.clone();
        let target = self.focus.other();
        let list = self.refs(target);
        if list.is_empty() {
            return Effect::None;
        }
        let idx = position(&list, self.selected.as_ref()).unwrap_or(0);
        self.selected = list.get(idx).cloned();
        self.focus = target;
        self.hint = idx;
        self.select_effect(before)
    }
}
