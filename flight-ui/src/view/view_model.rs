// SPDX-License-Identifier: MIT

use super::lists::workspaces;
use super::{
    Action, Effect, FilterInput, FormOutcome, HostChoice, InputMode, NewSessionForm,
    NewSessionRequest, NewSurfaceRequest, PromptOutcome, ShellPrompt, SurfaceChoice,
};
use crate::collect::CreateFailure;
use crate::snapshot::HostHealth;
use crate::snapshot::{PanePreview, UiSnapshot, Workspace, WorkspaceKey};
use flight_state::{HostId, PaneRef};

/// Presentation state: the latest snapshot plus what the user is pointing at. Selection is
/// the identity of a workspace, never a row number, so reordering cannot move the cursor onto
/// a different workspace; the visible row is derived from it each frame.
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub(super) snapshot: UiSnapshot,
    pub(super) selected: Option<WorkspaceKey>,
    /// Index the selection had in the list, to pick a neighbour if it vanishes.
    pub(super) hint: usize,
    pub(super) preview: Option<PanePreview>,
    pub(super) message: Option<String>,
    pub(super) loaded: bool,
    /// The new-session form, while it is open.
    pub(super) form: Option<NewSessionForm>,
    /// The companion-shell prompt, while it is open.
    pub(super) prompt: Option<ShellPrompt>,
    /// A workspace just created: select it when it shows up in a snapshot.
    pub(super) pending: Option<Pending>,
    /// A surface to open as soon as it shows up in a snapshot (just created, or asked for by
    /// a terminal that was left to switch).
    pub(super) opening: Option<Opening>,
    /// Where to put the cursor once that workspace is listed: the snapshots before the
    /// connection is up are empty, so this waits for it.
    pub(super) pointing: Option<(WorkspaceKey, u8)>,
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
pub(super) struct Opening {
    pub(super) key: WorkspaceKey,
    pub(super) choice: SurfaceChoice,
    /// The workspace is there but lacks the surface: offer to make a shell (true, for a
    /// terminal that was left to switch) or keep waiting for it to be published (false, for one
    /// just created).
    pub(super) offer_if_missing: bool,
    pub(super) snapshots_left: u8,
}

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
            prompt: None,
            pending: None,
            opening: None,
            pointing: None,
            filter: String::new(),
            searching: false,
            help: false,
            tick: 0,
        }
    }

    pub fn snapshot(&self) -> &UiSnapshot {
        &self.snapshot
    }

    /// The pane the selected workspace is previewed by (its agent's, or its shell's).
    pub fn selected(&self) -> Option<PaneRef> {
        self.selected_workspace()?
            .anchor_pane()
            .map(|p| p.pane_ref.clone())
    }

    pub fn selected_key(&self) -> Option<&WorkspaceKey> {
        self.selected.as_ref()
    }

    /// The workspaces on screen: most urgent first, narrowed by the search.
    pub fn listed(&self) -> Vec<Workspace> {
        workspaces(&self.snapshot, &self.filter)
    }

    pub fn selected_workspace(&self) -> Option<Workspace> {
        let want = self.selected.as_ref()?;
        self.listed().into_iter().find(|w| &w.key() == want)
    }

    /// The companion-shell prompt, if it is open.
    pub fn prompt(&self) -> Option<&ShellPrompt> {
        self.prompt.as_ref()
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
        } else if self.prompt.is_some() {
            InputMode::Prompt
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
            .filter(|p| Some(&p.pane) == self.selected().as_ref())
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
        let before = self.selected();
        self.reconcile();
        self.apply_pointing();
        self.select_created();
        let opened = self.open_pending();
        if matches!(opened, Effect::None) {
            self.select_effect(before)
        } else {
            opened
        }
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
            Action::Select(pane) => self.select(&pane),
            Action::Open(choice) => self.open(choice),
            Action::Prompt(input) => self.apply_prompt(input),
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
            Action::Switch => self.open(SurfaceChoice::Agent),
            Action::NewSession => {
                self.open_form();
                Effect::None
            }
            Action::Form(_) => Effect::None,
        }
    }

    fn open_form(&mut self) {
        let prefer = self.selected.as_ref().map(|k| k.host.clone());
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
                    "Created workspace {} on {}.",
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
        let now = self.selected();
        if before == now {
            Effect::None
        } else {
            Effect::Select(now)
        }
    }

    pub(super) fn keys(&self) -> Vec<WorkspaceKey> {
        self.listed().iter().map(Workspace::key).collect()
    }

    /// Open a surface of the selected workspace. The agent is the default (Enter); a workspace
    /// whose agent is gone opens its shell. A shell that does not exist yet is offered, not
    /// silently made.
    fn open(&mut self, choice: SurfaceChoice) -> Effect {
        self.pending = None;
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
    fn apply_prompt(&mut self, input: super::PromptInput) -> Effect {
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

    fn apply_pointing(&mut self) {
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
    fn open_pending(&mut self) -> Effect {
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
