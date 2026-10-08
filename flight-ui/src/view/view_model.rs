// SPDX-License-Identifier: MIT

use super::lists::sessions;
use super::{
    Action, Effect, FilterInput, FormOutcome, HostChoice, InputMode, NewSessionForm,
    NewSessionRequest,
};
use crate::collect::CreateFailure;
use crate::snapshot::HostHealth;
use crate::snapshot::{PanePreview, PaneView, UiSnapshot};
use flight_state::{HostId, PaneRef};

/// Presentation state: the latest snapshot plus what the user is pointing at. Selection is
/// the identity of a pane, never a row number, so reordering cannot move the cursor onto a
/// different agent; the visible row is derived from it each frame.
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub(super) snapshot: UiSnapshot,
    pub(super) selected: Option<PaneRef>,
    /// Index the selection had in the list, to pick a neighbour if it vanishes.
    pub(super) hint: usize,
    pub(super) preview: Option<PanePreview>,
    pub(super) message: Option<String>,
    pub(super) loaded: bool,
    /// The new-session form, while it is open.
    pub(super) form: Option<NewSessionForm>,
    /// A session just created: select its pane when it shows up in a snapshot.
    pub(super) pending: Option<Pending>,
    /// The search text; only sessions matching it are listed.
    pub(super) filter: String,
    /// Keys are going into the search.
    pub(super) searching: bool,
    pub(super) help: bool,
    /// Advances with the terminal loop; animates the working spinner.
    pub(super) tick: u32,
}

/// How many snapshots to wait for a created session to show up before giving up on selecting it.
const PENDING_SNAPSHOTS: u8 = 30;

#[derive(Debug, Clone)]
pub(super) struct Pending {
    pub(super) host: HostId,
    pub(super) session: String,
    pub(super) snapshots_left: u8,
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
            hint: 0,
            preview: None,
            message: None,
            loaded: false,
            form: None,
            pending: None,
            filter: String::new(),
            searching: false,
            help: false,
            tick: 0,
        }
    }

    pub fn snapshot(&self) -> &UiSnapshot {
        &self.snapshot
    }

    pub fn selected(&self) -> Option<&PaneRef> {
        self.selected.as_ref()
    }

    /// The sessions on screen: most urgent first, narrowed by the search.
    pub fn listed(&self) -> Vec<&PaneView> {
        sessions(&self.snapshot, &self.filter)
    }

    fn selected_view(&self) -> Option<&PaneView> {
        let want = self.selected.as_ref()?;
        self.snapshot
            .hosts
            .iter()
            .flat_map(|h| h.panes.iter())
            .find(|p| &p.pane_ref == want)
    }

    /// The search text (empty: no filter).
    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn searching(&self) -> bool {
        self.searching
    }

    pub fn help_open(&self) -> bool {
        self.help
    }

    pub fn input_mode(&self) -> InputMode {
        if self.form.is_some() {
            InputMode::Form
        } else if self.help {
            InputMode::Help
        } else if self.searching {
            InputMode::Search
        } else {
            InputMode::Dashboard
        }
    }

    /// Advance the animation one step.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn spinner_frame(&self) -> u32 {
        self.tick
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// The new-session form, if it is open.
    pub fn form(&self) -> Option<&NewSessionForm> {
        self.form.as_ref()
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
        self.select_created();
        self.select_effect(before)
    }

    pub fn apply(&mut self, action: Action) -> Effect {
        if let Action::Form(input) = action {
            return self.apply_form(input);
        }
        match action {
            Action::Quit => Effect::Quit,
            Action::Back if self.filter.is_empty() => Effect::Quit,
            Action::Back => self.filter_input(FilterInput::Clear),
            Action::Refresh => Effect::Refresh,
            Action::Up => self.step(-1),
            Action::Down => self.step(1),
            Action::Select(pane) => self.select(pane),
            Action::Search => {
                self.searching = true;
                Effect::None
            }
            Action::Filter(input) => self.filter_input(input),
            Action::Help => {
                self.help = true;
                Effect::None
            }
            Action::CloseHelp => {
                self.help = false;
                Effect::None
            }
            Action::Switch => self
                .selected_view()
                .cloned()
                .map_or(Effect::None, Effect::Switch),
            Action::NewSession => {
                self.open_form();
                Effect::None
            }
            Action::Form(_) => Effect::None,
        }
    }

    fn open_form(&mut self) {
        let prefer = self.selected.as_ref().map(|p| p.host.clone());
        self.form = Some(NewSessionForm::new(self.connected_hosts(), prefer.as_ref()));
    }

    /// The nodes a session can be created on: those the orchestrator has a live link to.
    /// A node with no sessions yet is included, since creating one is how it gets its first.
    fn connected_hosts(&self) -> Vec<HostChoice> {
        let mut out: Vec<HostChoice> = Vec::new();
        for h in &self.snapshot.hosts {
            if matches!(h.health, HostHealth::Online | HostHealth::NoServer)
                && !out.iter().any(|c| c.host == h.host)
            {
                out.push(HostChoice {
                    host: h.host.clone(),
                    label: h.label.clone(),
                });
            }
        }
        out
    }

    fn apply_form(&mut self, input: super::FormInput) -> Effect {
        let Some(form) = self.form.as_mut() else {
            return Effect::None;
        };
        match form.handle(input) {
            FormOutcome::None => Effect::None,
            FormOutcome::Cancel => {
                self.form = None;
                Effect::None
            }
            FormOutcome::Submit(request) => Effect::Create(request),
        }
    }

    /// The answer to an [`Effect::Create`]. Success closes the form and aims the selection at
    /// the new session; a refusal stays in the form, where it can be fixed.
    pub fn apply_created(
        &mut self,
        request: &NewSessionRequest,
        result: Result<(), CreateFailure>,
    ) {
        match result {
            Ok(()) => {
                self.form = None;
                self.pending = Some(Pending {
                    host: request.host.clone(),
                    session: request.name.clone(),
                    snapshots_left: PENDING_SNAPSHOTS,
                });
                self.message = Some(format!(
                    "Created session {} on {}.",
                    request.name, request.host_label
                ));
                self.select_created();
            }
            Err(failure) => match self.form.as_mut() {
                Some(form) => form.fail(&failure),
                None => {
                    self.message = Some(format!("Could not create {}: {failure:?}", request.name))
                }
            },
        }
    }

    pub(super) fn select_effect(&self, before: Option<PaneRef>) -> Effect {
        if before == self.selected {
            Effect::None
        } else {
            Effect::Select(self.selected.clone())
        }
    }

    pub(super) fn refs(&self) -> Vec<PaneRef> {
        sessions(&self.snapshot, &self.filter)
            .into_iter()
            .map(|p| p.pane_ref.clone())
            .collect()
    }
}
