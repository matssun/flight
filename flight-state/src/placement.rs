// SPDX-License-Identifier: MIT

use crate::{valid_id, HostId, ServerId, SurfaceId, WorkspaceId};

/// What a surface is for, as the backend records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    Agent,
    Shell,
}

impl SurfaceRole {
    /// The role a marker names, or `None` for an absent or unknown value (a window that was
    /// never marked, or marked by a newer Flight).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

/// What the backend recorded about where a pane belongs, exactly as read: Flight's own
/// markers (empty when the session predates workspaces) and the session's identity and start
/// directory. Turning this into a workspace and a surface is [`Self::workspace`],
/// [`Self::surface`] and [`Self::role`], the same for every reader of the backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawPlacement {
    pub workspace_id: String,
    pub surface_id: String,
    pub surface_kind: String,
    pub window_id: String,
    pub session_id: String,
    pub session_path: String,
}

impl RawPlacement {
    /// The workspace the pane is a surface of: the one Flight recorded, or, for a session that
    /// predates workspaces, one derived from the host and the session's backend identity.
    pub fn workspace(&self, host: &HostId, server: &ServerId) -> WorkspaceId {
        if valid_id(&self.workspace_id) {
            WorkspaceId::new(&self.workspace_id)
        } else {
            WorkspaceId::for_unmarked_session(host.as_str(), server.as_str(), &self.session_id)
        }
    }

    /// What the pane is for. A window Flight marked says so; an unmarked one is an agent if it
    /// runs one and a shell if it is only published because its session is Flight's.
    pub fn role(&self, runs_agent: bool) -> SurfaceRole {
        SurfaceRole::parse(&self.surface_kind).unwrap_or(if runs_agent {
            SurfaceRole::Agent
        } else {
            SurfaceRole::Shell
        })
    }

    /// The surface's id: the recorded one, or one derived from the window for an unmarked pane.
    pub fn surface(&self, host: &HostId, server: &ServerId) -> SurfaceId {
        if valid_id(&self.surface_id) {
            SurfaceId::new(&self.surface_id)
        } else {
            SurfaceId::new(format!(
                "{}.w{}",
                self.workspace(host, server),
                self.window_id.trim_start_matches('@')
            ))
        }
    }

    /// The workspace's root directory: where its session was started.
    pub fn root<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.session_path.is_empty() {
            fallback
        } else {
            &self.session_path
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(ws: &str, surface: &str, kind: &str) -> RawPlacement {
        RawPlacement {
            workspace_id: ws.into(),
            surface_id: surface.into(),
            surface_kind: kind.into(),
            window_id: "@5".into(),
            session_id: "$3".into(),
            session_path: "/work".into(),
        }
    }

    fn where_() -> (HostId, ServerId) {
        (HostId::new("ab12cd34ef"), ServerId::new("work"))
    }

    #[test]
    fn recorded_ids_are_used_as_they_are() {
        let (host, server) = where_();
        let p = placed("w-1", "s-2", "shell");
        assert_eq!(p.workspace(&host, &server).as_str(), "w-1");
        assert_eq!(p.surface(&host, &server).as_str(), "s-2");
        assert_eq!(p.role(true), SurfaceRole::Shell);
    }

    #[test]
    fn an_unmarked_pane_gets_ids_that_are_neither_its_name_nor_a_pane_id() {
        let (host, server) = where_();
        let p = placed("", "", "");
        assert_eq!(
            p.workspace(&host, &server).as_str(),
            "legacy.ab12cd34.work.3"
        );
        assert_eq!(
            p.surface(&host, &server).as_str(),
            "legacy.ab12cd34.work.3.w5"
        );
    }

    #[test]
    fn an_unmarked_pane_is_an_agent_if_it_runs_one_and_a_shell_otherwise() {
        let p = placed("", "", "");
        assert_eq!(p.role(true), SurfaceRole::Agent);
        assert_eq!(p.role(false), SurfaceRole::Shell);
    }

    #[test]
    fn a_malformed_recorded_id_is_ignored_rather_than_trusted() {
        let (host, server) = where_();
        let p = placed("bad id", "also bad", "agent");
        assert_eq!(
            p.workspace(&host, &server).as_str(),
            "legacy.ab12cd34.work.3"
        );
        assert!(p.surface(&host, &server).as_str().starts_with("legacy."));
    }

    #[test]
    fn the_root_is_the_session_start_directory_when_known() {
        assert_eq!(placed("w", "s", "agent").root("/elsewhere"), "/work");
        assert_eq!(RawPlacement::default().root("/elsewhere"), "/elsewhere");
    }
}
