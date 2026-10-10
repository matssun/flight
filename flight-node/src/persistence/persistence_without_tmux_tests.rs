// SPDX-License-Identifier: MIT

//! The saved workspaces driven through `NodeTmux` by a node that has no tmux: what persistence
//! asks of the node is exactly the three things in the interface.

use super::{NodeTmux, WorkspacePersistence};
use crate::agent_resume::{AgentLaunch, AgentSession};
use crate::{ControlError, Program, SessionRequest};
use flight_state::{HostId, ServerId};
use flight_tmux::{ConfigMark, PaneInfo, SurfaceMark, SurfaceTag};
use flight_workspaces::{ConfigKey, RecoveryPolicy};
use std::sync::Mutex;

#[derive(Default)]
struct EmptyNode {
    /// The saved workspace each requested session was for.
    started: Mutex<Vec<String>>,
}

impl NodeTmux for EmptyNode {
    fn published_panes(&self) -> Result<Vec<(ServerId, PaneInfo)>, String> {
        Ok(Vec::new())
    }

    fn create_session_marked(
        &self,
        _request: &SessionRequest,
        config: Option<ConfigMark>,
        _agent: &AgentLaunch,
    ) -> Result<(ServerId, SurfaceMark, Option<AgentSession>), ControlError> {
        let config = config.expect("persistence marks what it starts");
        self.started.lock().unwrap().push(config.workspace.clone());
        let mark = SurfaceMark {
            workspace_id: "w-new".to_owned(),
            surface_id: "s-new".to_owned(),
            kind: SurfaceTag::Agent,
            config: Some(config),
        };
        Ok((ServerId::new("fake"), mark, None))
    }

    fn create_shell_marked(
        &self,
        _host: &HostId,
        _workspace_id: &str,
        _surface: &ConfigKey,
    ) -> Result<(SurfaceMark, String), ControlError> {
        unreachable!("a saved workspace with only an agent needs no shell")
    }
}

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("flight-nodetmux-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_saved_workspace_that_is_not_running_is_started_through_the_interface_and_only_then() {
    let root = dir("root");
    let persistence = WorkspacePersistence::open(&dir("saved"), "host", Some(root.clone()));
    let config = ConfigMark {
        workspace: ConfigKey::mint().unwrap().to_string(),
        surface: ConfigKey::mint().unwrap().to_string(),
    };
    let mark = SurfaceMark {
        workspace_id: "w-old".to_owned(),
        surface_id: "s-old".to_owned(),
        kind: SurfaceTag::Agent,
        config: Some(config.clone()),
    };
    let request = SessionRequest {
        name: "api".to_owned(),
        dir: root.to_string_lossy().into_owned(),
        program: Program::Claude,
    };
    persistence
        .record_session(&request, &mark, &config, None)
        .unwrap();

    let node = EmptyNode::default();
    // Looking changes nothing.
    assert_eq!(persistence.report(&node).len(), 1);
    assert!(node.started.lock().unwrap().is_empty());

    // Not asked to start what is missing: nothing is started.
    persistence
        .recover(&node, &RecoveryPolicy::default())
        .unwrap()
        .unwrap();
    assert!(node.started.lock().unwrap().is_empty());

    // Asked to: the workspace is started, for the saved definition, through the interface.
    let policy = RecoveryPolicy {
        start_missing: true,
        ..RecoveryPolicy::default()
    };
    persistence.recover(&node, &policy).unwrap().unwrap();
    assert_eq!(*node.started.lock().unwrap(), [config.workspace]);
}
