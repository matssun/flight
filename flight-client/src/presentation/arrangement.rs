// SPDX-License-Identifier: MIT

use crate::LayoutStore;
use flight_present::{saved, Axis, Layout, Placement};
use flight_state::SurfaceId;
use flight_ui::WorkspaceKey;

/// The agent and the shell side by side, with the keyboard on `focus` (or the agent).
pub fn side_by_side(focus: &SurfaceId) -> Layout {
    let agent = SurfaceId::new("agent");
    let shell = SurfaceId::new("shell");
    let layout = Layout::single(agent.clone())
        .split(&agent, Axis::Across, shell.clone(), Placement::After)
        .unwrap_or_else(|_| Layout::single(agent.clone()));
    layout.focus_on(focus).unwrap_or(layout)
}

/// The arrangement to start with: what the user left last time if it still fits the surfaces
/// that exist (anything it names that does not exist is dropped, and what is left must still be
/// a valid layout), otherwise the surfaces side by side.
pub fn starting_layout(
    store: Option<&LayoutStore>,
    workspace: &WorkspaceKey,
    focus: &SurfaceId,
    exists: impl Fn(&SurfaceId) -> bool,
) -> Layout {
    store
        .and_then(|s| s.get(workspace))
        .and_then(|text| saved::from_toml(text, exists).ok())
        // The keyboard is where the user was, if that surface is showing.
        .map(|layout| layout.focus_on(focus).unwrap_or(layout))
        .unwrap_or_else(|| side_by_side(focus))
}

/// Remember the arrangement the user left.
pub fn remember(
    store: &mut LayoutStore,
    workspace: &WorkspaceKey,
    layout: &Layout,
) -> Result<(), String> {
    let text = saved::to_toml(layout).map_err(|e| e.to_string())?;
    store.put(workspace, text);
    store.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flight_state::{HostId, WorkspaceId};

    fn key() -> WorkspaceKey {
        WorkspaceKey {
            host: HostId::new("h"),
            workspace: WorkspaceId::new("w"),
        }
    }

    #[test]
    fn the_default_has_both_surfaces_and_the_keyboard_where_the_user_was() {
        let shell = SurfaceId::new("shell");
        let layout = side_by_side(&shell);
        assert_eq!(layout.surfaces().len(), 2);
        assert_eq!(layout.focus(), &shell);
    }

    #[test]
    fn a_remembered_arrangement_is_used_only_if_it_still_fits() {
        let dir = std::env::temp_dir().join(format!("flight-arrange-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = LayoutStore::open(&dir).unwrap();
        let tabs = Layout::single(SurfaceId::new("agent"))
            .add_tab(&SurfaceId::new("agent"), SurfaceId::new("shell"))
            .unwrap();
        remember(&mut store, &key(), &tabs).unwrap();
        let store = LayoutStore::open(&dir).unwrap();
        let focus = SurfaceId::new("agent");
        let back = starting_layout(Some(&store), &key(), &focus, |_| true);
        // The keyboard is where the user was, which shows that tab.
        assert_eq!(back, tabs.focus_on(&focus).unwrap());
        // The shell no longer exists: the remembered layout is cut down to what does.
        let cut = starting_layout(Some(&store), &key(), &focus, |s| s.as_str() == "agent");
        assert_eq!(cut.surfaces().len(), 1);
        // Nothing remembered: the default.
        let none = starting_layout(None, &key(), &focus, |_| true);
        assert_eq!(none, side_by_side(&focus));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
