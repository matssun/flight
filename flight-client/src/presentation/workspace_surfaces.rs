// SPDX-License-Identifier: MIT

use super::surface_names::surface_id;
use super::target::Target;
use flight_state::SurfaceId;
use flight_ui::{SurfaceChoice, SurfaceKind, Workspace};

/// One surface of a workspace, as a layout names it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: SurfaceId,
    target: Target,
    label: String,
}

/// The surfaces a workspace has, and what a layout calls each. The first agent is `agent` and
/// the first shell is `shell` (the names layouts have always used, which keep meaning "the
/// workspace's agent" when its window is replaced); every other surface is named by its own
/// id, and so can be told apart and kept in a saved layout for as long as it exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSurfaces {
    entries: Vec<Entry>,
}

impl WorkspaceSurfaces {
    /// What the workspace has now, as the dashboard lists it.
    pub fn of(workspace: &Workspace) -> Self {
        let mut agent_taken = false;
        let mut shell_taken = false;
        let mut entries = Vec::new();
        for surface in &workspace.surfaces {
            let (choice, taken, fixed) = match surface.kind {
                SurfaceKind::Agent(_) => (SurfaceChoice::Agent, &mut agent_taken, "agent"),
                SurfaceKind::Shell => (SurfaceChoice::Shell, &mut shell_taken, "shell"),
            };
            let (name, target) = if *taken {
                (
                    surface.id.clone(),
                    Target {
                        choice,
                        surface: Some(surface.id.clone()),
                    },
                )
            } else {
                *taken = true;
                (
                    SurfaceId::new(fixed),
                    Target {
                        choice,
                        surface: None,
                    },
                )
            };
            let label = if target.surface.is_some() {
                surface.pane.window.clone()
            } else {
                fixed.to_owned()
            };
            entries.push(Entry {
                name,
                target,
                label: if label.is_empty() {
                    surface.id.to_string()
                } else {
                    label
                },
            });
        }
        // The names every workspace is expected to have come first, in their usual order.
        entries.sort_by_key(|e| match e.name.as_str() {
            "agent" => 0,
            "shell" => 1,
            _ => 2,
        });
        Self { entries }
    }

    /// A workspace's agent and shell, by their usual names: what to assume when nothing is
    /// known of the workspace.
    pub fn standard() -> Self {
        let entries = [SurfaceChoice::Agent, SurfaceChoice::Shell]
            .into_iter()
            .map(|choice| Entry {
                name: surface_id(choice),
                label: surface_id(choice).to_string(),
                target: Target {
                    choice,
                    surface: None,
                },
            })
            .collect();
        Self { entries }
    }

    /// The names, in the order a new split takes them.
    pub fn names(&self) -> Vec<SurfaceId> {
        self.entries.iter().map(|e| e.name.clone()).collect()
    }

    pub fn target(&self, name: &SurfaceId) -> Option<Target> {
        self.entries
            .iter()
            .find(|e| &e.name == name)
            .map(|e| e.target.clone())
            // The agent and the shell can always be asked for, even when the workspace has
            // none now: the tile says so, and the surface may come back.
            .or_else(|| {
                super::surface_names::surface_choice(name).map(|choice| Target {
                    choice,
                    surface: None,
                })
            })
    }

    pub fn label(&self, name: &SurfaceId) -> String {
        self.entries
            .iter()
            .find(|e| &e.name == name)
            .map_or_else(|| name.to_string(), |e| e.label.clone())
    }

    /// Whether a saved layout may keep a surface called `name`: the agent and the shell always
    /// (a surface that is down for a moment is still the user's arrangement), the others
    /// while they exist.
    pub fn is_known(&self, name: &SurfaceId) -> bool {
        self.target(name).is_some()
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use flight_classify::AgentKind;
    use flight_state::{AgentState, HostId, PaneId, PaneRef, ServerId, SurfaceId, WorkspaceId};
    use flight_ui::{HostHealth, HostView, PaneView, SurfaceKind, UiSnapshot, WorkspaceKey};

    pub(crate) fn key() -> WorkspaceKey {
        WorkspaceKey {
            host: HostId::new("h"),
            workspace: WorkspaceId::new("w"),
        }
    }

    /// A pane of workspace `w` on host `h`: `id` is its surface id, `window` its window name.
    pub(crate) fn pane(id: &str, kind: SurfaceKind, window: &str, pid: u32) -> PaneView {
        PaneView {
            pane_ref: PaneRef {
                host: HostId::new("h"),
                server: ServerId::new("s"),
                pane: PaneId::new(format!("%{pid}")),
            },
            session: "w".to_owned(),
            window: window.to_owned(),
            agent: AgentKind::Other,
            state: AgentState::Idle,
            why: String::new(),
            title: String::new(),
            pid,
            workspace: WorkspaceId::new("w"),
            surface: SurfaceId::new(id),
            kind,
            root: "/work".to_owned(),
        }
    }

    pub(crate) fn snapshot(panes: Vec<PaneView>) -> UiSnapshot {
        UiSnapshot {
            hosts: vec![HostView {
                host: HostId::new("h"),
                label: "h".to_owned(),
                server: ServerId::new("s"),
                health: HostHealth::Online,
                panes,
            }],
            ..UiSnapshot::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use flight_classify::AgentKind;
    use flight_ui::workspaces;

    fn surfaces_of(panes: Vec<flight_ui::PaneView>) -> WorkspaceSurfaces {
        let snapshot = snapshot(panes);
        let all = workspaces(&snapshot, "");
        WorkspaceSurfaces::of(&all[0])
    }

    #[test]
    fn the_first_agent_and_shell_keep_their_names_and_every_other_surface_is_its_own() {
        let found = surfaces_of(vec![
            pane("s-3", SurfaceKind::Shell, "logs", 3),
            pane("s-1", SurfaceKind::Agent(AgentKind::Claude), "agent", 1),
            pane("s-2", SurfaceKind::Shell, "shell", 2),
        ]);
        let names: Vec<String> = found.names().iter().map(ToString::to_string).collect();
        assert_eq!(names, ["agent", "shell", "s-3"]);
        // The agent and shell are found by kind; the other by its own id.
        let other = found.target(&SurfaceId::new("s-3")).unwrap();
        assert_eq!(other.surface, Some(SurfaceId::new("s-3")));
        assert_eq!(other.choice, SurfaceChoice::Shell);
        assert_eq!(
            found.target(&SurfaceId::new("shell")).unwrap().surface,
            None
        );
        // A tab says what the window is called.
        assert_eq!(found.label(&SurfaceId::new("s-3")), "logs");
        assert_eq!(found.label(&SurfaceId::new("agent")), "agent");
    }

    #[test]
    fn a_saved_layout_keeps_the_agent_and_shell_always_and_the_others_while_they_exist() {
        let found = surfaces_of(vec![pane("s-1", SurfaceKind::Shell, "sh", 1)]);
        assert!(found.is_known(&SurfaceId::new("agent")));
        assert!(found.is_known(&SurfaceId::new("shell")));
        assert!(!found.is_known(&SurfaceId::new("s-gone")));
        let two = surfaces_of(vec![
            pane("s-1", SurfaceKind::Shell, "sh", 1),
            pane("s-2", SurfaceKind::Shell, "logs", 2),
        ]);
        assert!(two.is_known(&SurfaceId::new("s-2")));
    }

    #[test]
    fn a_workspace_nothing_is_known_of_has_the_standard_two() {
        let names: Vec<String> = WorkspaceSurfaces::standard()
            .names()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(names, ["agent", "shell"]);
    }
}
