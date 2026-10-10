// SPDX-License-Identifier: MIT

use crate::session::{
    Attachment, Binding, FromRemote, OpenFailure, OpenRequest, SurfaceHost, ToRemote,
};
use crate::terminal::terminal_request_shape;
use crate::OrchestratedBackend;
use flight_proto::{
    terminal_body, TerminalClose, TerminalFrame, TerminalResize, MAX_TERMINAL_DATA,
};
use flight_state::SurfaceId;
use flight_transport::TerminalReceiver;
use flight_ui::{workspaces, SurfaceChoice, UiSnapshot, WorkspaceKey};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Frames queued each way between an attachment's stream and the session.
const QUEUE: usize = 4;
/// How long a stream the session has let go of is kept so the far end can read the goodbye.
const GOODBYE: Duration = Duration::from_secs(2);

/// The surfaces of one workspace, reached over the link the process already holds: no new
/// control connection, no reveal, and the orchestrator's own checks (identity, limits, the pid
/// guard) on every terminal it is asked for.
pub struct LinkHost {
    link: OrchestratedBackend,
    workspace: WorkspaceKey,
}

impl LinkHost {
    pub fn new(link: OrchestratedBackend, workspace: WorkspaceKey) -> Self {
        Self { link, workspace }
    }

    /// The pane showing the wanted surface of this workspace, as the link last saw the fleet:
    /// `surface` if one is named, else the workspace's agent or shell (`choice`). When `expect`
    /// is set only that exact process will do.
    fn resolve(
        &self,
        choice: SurfaceChoice,
        surface: Option<&SurfaceId>,
        expect: Option<&Binding>,
    ) -> Result<Binding, OpenFailure> {
        let found = find(
            &self.link.current_snapshot(),
            &self.workspace,
            choice,
            surface,
        );
        match (found, expect) {
            (Some(now), Some(then)) if now != *then => Err(OpenFailure::Refused(format!(
                "the {} changed while it was disconnected",
                choice.label()
            ))),
            (Some(now), _) => Ok(now),
            // The link may simply not have the fleet yet (it is reconnecting).
            (None, _) if !self.link.connected() => Err(OpenFailure::Unavailable(
                "not connected to the orchestrator".to_owned(),
            )),
            (None, _) => Err(OpenFailure::Refused(format!(
                "this workspace has no {} now",
                choice.label()
            ))),
        }
    }
}

/// The pane of a surface of the workspace `key`: the one called `surface` if given, else the
/// workspace's agent or shell, in the order the dashboard lists them.
fn find(
    snapshot: &UiSnapshot,
    key: &WorkspaceKey,
    choice: SurfaceChoice,
    surface: Option<&SurfaceId>,
) -> Option<Binding> {
    let workspace = workspaces(snapshot, "")
        .into_iter()
        .find(|w| &w.key() == key)?;
    let found = match surface {
        Some(id) => workspace
            .surfaces
            .iter()
            .find(|s| &s.id == id && choice.is(s.kind)),
        None => match choice {
            SurfaceChoice::Agent => workspace.agent(),
            SurfaceChoice::Shell => workspace.shell(),
        },
    }?;
    Some(Binding {
        pane: found.pane.pane_ref.clone(),
        pid: found.pane.pid,
    })
}

impl SurfaceHost for LinkHost {
    async fn open(&self, request: OpenRequest) -> Result<Attachment, OpenFailure> {
        let binding = self.resolve(
            request.choice,
            request.surface.as_ref(),
            request.expect.as_ref(),
        )?;
        let (_, _, term) = terminal_request_shape();
        let id = self
            .link
            .request_terminal(
                &binding.pane,
                binding.pid,
                (request.cols, request.rows, term),
            )
            .await?;
        self.connect(id, request.choice, binding).await
    }

    async fn connect(
        &self,
        id: Vec<u8>,
        _choice: SurfaceChoice,
        binding: Binding,
    ) -> Result<Attachment, OpenFailure> {
        let client = self
            .link
            .terminals()
            .connect_ui(&id)
            .await
            .map_err(|e| OpenFailure::Unavailable(e.to_string()))?;
        let (sender, mut receiver) = client.split();
        let (to_remote, mut commands) = mpsc::channel::<ToRemote>(QUEUE);
        let (reports, from_remote) = mpsc::channel::<FromRemote>(QUEUE);
        let (retired_tx, retired) = oneshot::channel::<()>();

        // Stream to session. Ends with the stream, or when the session stops listening.
        tokio::spawn(async move {
            // The attachment is retired when this task ends, however it ends: the far end has
            // then read what was sent and finished the stream (or the stream broke).
            let _retired = retired_tx;
            let end = loop {
                match receiver.next().await {
                    Ok(Some(frame)) => match frame.body {
                        Some(terminal_body::Body::Data(d)) => {
                            if reports.send(FromRemote::Data(d.payload)).await.is_err() {
                                // The session let go. Keep the stream until the far end has read
                                // what was sent and ended it: dropping it now would cancel the
                                // stream with the goodbye (and the last input) still unsent.
                                finish_reading(receiver).await;
                                return;
                            }
                        }
                        Some(terminal_body::Body::Exit(e)) => {
                            break FromRemote::Exit {
                                reason: flight_proto::ExitReasonCode::try_from(e.reason)
                                    .unwrap_or(flight_proto::ExitReasonCode::Unspecified),
                                status: e.status,
                            };
                        }
                        _ => {}
                    },
                    Ok(None) => break FromRemote::Lost("the stream ended".to_owned()),
                    Err(e) => break FromRemote::Lost(e.to_string()),
                }
            };
            let _ = reports.send(end).await;
        });

        // Session to stream. When the session lets go, the stream is half-closed after the
        // goodbye, so the node reads it before the stream is dropped.
        tokio::spawn(async move {
            while let Some(command) = commands.recv().await {
                let frame = match command {
                    ToRemote::Data(bytes) => {
                        let mut ok = true;
                        for chunk in bytes.chunks(MAX_TERMINAL_DATA) {
                            ok &= sender
                                .send(TerminalFrame::data(chunk.to_vec()))
                                .await
                                .is_ok();
                        }
                        if ok {
                            continue;
                        }
                        return;
                    }
                    ToRemote::Resize(cols, rows) => TerminalFrame {
                        body: Some(terminal_body::Body::Resize(TerminalResize {
                            cols: u32::from(cols).clamp(1, flight_proto::MAX_TERMINAL_DIM),
                            rows: u32::from(rows).clamp(1, flight_proto::MAX_TERMINAL_DIM),
                        })),
                    },
                    ToRemote::Close => {
                        let _ = sender
                            .send(TerminalFrame {
                                body: Some(terminal_body::Body::Close(TerminalClose {})),
                            })
                            .await;
                        return;
                    }
                };
                if sender.send(frame).await.is_err() {
                    return;
                }
            }
        });
        Ok(Attachment {
            id,
            binding,
            to_remote,
            from_remote,
            guard: None,
            retired: Some(retired),
        })
    }

    fn renew(&self, attachment: &[u8]) -> Result<(), String> {
        self.link.send_lease(attachment)
    }
}

/// Read a stream to its end (or for [`GOODBYE`]), discarding what it says.
async fn finish_reading(mut receiver: TerminalReceiver) {
    let _ = tokio::time::timeout(GOODBYE, async {
        while let Ok(Some(_)) = receiver.next().await {}
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::workspace_surfaces::fixtures::*;
    use flight_classify::AgentKind;
    use flight_ui::SurfaceKind;

    #[test]
    fn the_agent_and_shell_are_found_by_kind_and_any_other_surface_by_its_own_id() {
        let snapshot = snapshot(vec![
            pane("s-3", SurfaceKind::Shell, "logs", 3),
            pane("s-1", SurfaceKind::Agent(AgentKind::Claude), "agent", 1),
            pane("s-2", SurfaceKind::Shell, "shell", 2),
        ]);
        let pid = |choice, surface: Option<&str>| {
            find(
                &snapshot,
                &key(),
                choice,
                surface.map(SurfaceId::new).as_ref(),
            )
            .map(|b| b.pid)
        };
        assert_eq!(pid(SurfaceChoice::Agent, None), Some(1));
        // The first shell in the order the dashboard lists them.
        assert_eq!(pid(SurfaceChoice::Shell, None), Some(2));
        assert_eq!(pid(SurfaceChoice::Shell, Some("s-3")), Some(3));
        // Named, but not of that kind or not there: nothing, never another surface.
        assert_eq!(pid(SurfaceChoice::Agent, Some("s-3")), None);
        assert_eq!(pid(SurfaceChoice::Shell, Some("s-9")), None);
    }

    #[test]
    fn a_workspace_that_is_not_in_the_snapshot_has_nothing() {
        let other = WorkspaceKey {
            host: flight_state::HostId::new("h"),
            workspace: flight_state::WorkspaceId::new("nope"),
        };
        let snapshot = snapshot(vec![pane("s-2", SurfaceKind::Shell, "shell", 2)]);
        assert!(find(&snapshot, &other, SurfaceChoice::Shell, None).is_none());
    }
}
