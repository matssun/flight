// SPDX-License-Identifier: MIT

use crate::session::{
    Attachment, Binding, FromRemote, OpenFailure, OpenRequest, SurfaceHost, ToRemote,
};
use crate::terminal::terminal_request_shape;
use crate::OrchestratedBackend;
use flight_proto::{
    terminal_body, TerminalClose, TerminalFrame, TerminalResize, MAX_TERMINAL_DATA,
};
use flight_transport::TerminalReceiver;
use flight_ui::{SurfaceChoice, WorkspaceKey};
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

    /// The pane showing `choice` for this workspace, as the link last saw the fleet. When
    /// `expect` is set only that exact process will do.
    fn resolve(
        &self,
        choice: SurfaceChoice,
        expect: Option<&Binding>,
    ) -> Result<Binding, OpenFailure> {
        let snapshot = self.link.current_snapshot();
        let found = snapshot
            .hosts
            .iter()
            .filter(|h| h.host == self.workspace.host)
            .flat_map(|h| h.panes.iter())
            .filter(|p| p.workspace == self.workspace.workspace && choice.is(p.kind))
            .map(|p| Binding {
                pane: p.pane_ref.clone(),
                pid: p.pid,
            })
            .next();
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

impl SurfaceHost for LinkHost {
    async fn open(&self, request: OpenRequest) -> Result<Attachment, OpenFailure> {
        let binding = self.resolve(request.choice, request.expect.as_ref())?;
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
