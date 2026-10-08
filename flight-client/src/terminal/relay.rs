// SPDX-License-Identifier: MIT

use crate::terminal::{EscapeAction, EscapeFilter, TerminalEnd};
use flight_proto::{
    terminal_body, TerminalClose, TerminalFrame, TerminalResize, MAX_TERMINAL_DATA,
};
use flight_transport::{TerminalReceiver, TerminalSender};
use std::time::Duration;
use tokio::sync::mpsc;

/// How long to wait for the far end to finish reading a goodbye.
const GOODBYE: Duration = Duration::from_secs(1);

fn frame(body: terminal_body::Body) -> TerminalFrame {
    TerminalFrame { body: Some(body) }
}

/// Copy the user's keystrokes and size changes to the remote, and the remote's output to
/// `output`, until one side ends. The two directions are independent tasks so the local
/// escape works even when the user's terminal is not accepting output.
///
/// Bounds: this holds at most one output frame (16 KiB) in hand and at most the `output`
/// channel's capacity beyond it; input is forwarded in frames of at most 16 KiB.
pub async fn relay(
    sender: TerminalSender,
    mut receiver: TerminalReceiver,
    mut input: mpsc::Receiver<Vec<u8>>,
    mut resizes: mpsc::Receiver<(u16, u16)>,
    output: mpsc::Sender<Vec<u8>>,
    hint: impl Fn() + Send + 'static,
) -> TerminalEnd {
    let from_remote = tokio::spawn(async move {
        loop {
            match receiver.next().await {
                Ok(Some(f)) => match f.body {
                    Some(terminal_body::Body::Data(d)) => {
                        if output.send(d.payload).await.is_err() {
                            return TerminalEnd::UserLeft;
                        }
                    }
                    Some(terminal_body::Body::Exit(e)) => {
                        let reason = flight_proto::ExitReasonCode::try_from(e.reason)
                            .unwrap_or(flight_proto::ExitReasonCode::Unspecified);
                        return TerminalEnd::Exited {
                            reason,
                            status: e.status,
                        };
                    }
                    _ => {}
                },
                Ok(None) => return TerminalEnd::Lost("the stream ended".to_owned()),
                Err(e) => return TerminalEnd::Lost(e.to_string()),
            }
        }
    });
    let to_remote = tokio::spawn(async move {
        let mut filter = EscapeFilter::default();
        loop {
            tokio::select! {
                bytes = input.recv() => {
                    let Some(bytes) = bytes else {
                        return (TerminalEnd::UserLeft, sender);
                    };
                    let (forward, action) = filter.feed(&bytes);
                    for chunk in forward.chunks(MAX_TERMINAL_DATA) {
                        if sender.send(TerminalFrame::data(chunk.to_vec())).await.is_err() {
                            return (TerminalEnd::Lost("the stream is closed".to_owned()), sender);
                        }
                    }
                    match action {
                        EscapeAction::Leave => {
                            let _ = sender.send(frame(terminal_body::Body::Close(TerminalClose {}))).await;
                            return (TerminalEnd::UserLeft, sender);
                        }
                        EscapeAction::Hint => hint(),
                        EscapeAction::None => {}
                    }
                }
                size = resizes.recv() => {
                    if let Some((cols, rows)) = size {
                        let resize = frame(terminal_body::Body::Resize(TerminalResize {
                            cols: u32::from(cols).clamp(1, flight_proto::MAX_TERMINAL_DIM),
                            rows: u32::from(rows).clamp(1, flight_proto::MAX_TERMINAL_DIM),
                        }));
                        if sender.send(resize).await.is_err() {
                            return (TerminalEnd::Lost("the stream is closed".to_owned()), sender);
                        }
                    }
                }
            }
        }
    });
    tokio::pin!(from_remote);
    tokio::pin!(to_remote);
    tokio::select! {
        done = &mut from_remote => {
            to_remote.abort();
            done.unwrap_or_else(|e| TerminalEnd::Lost(e.to_string()))
        }
        done = &mut to_remote => {
            match done {
                Ok((end, sender)) => {
                    // Half-close, then let the goodbye be read before the stream is dropped.
                    drop(sender);
                    let _ = tokio::time::timeout(GOODBYE, &mut from_remote).await;
                    from_remote.abort();
                    end
                }
                Err(e) => {
                    from_remote.abort();
                    TerminalEnd::Lost(e.to_string())
                }
            }
        }
    }
}
