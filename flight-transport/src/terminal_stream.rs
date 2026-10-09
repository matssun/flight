// SPDX-License-Identifier: MIT

//! One end of a terminal stream on the orchestrator: authenticate, attach, then copy frames
//! to the other end through a bounded queue until either side ends.

use crate::shared::{lock, SharedState};
use crate::terminal_relay::Ends;
use crate::PeerIdentity;
use flight_orchestrator::Side;
use flight_proto::{
    terminal_body, ExitReasonCode, Origin, TerminalClose, TerminalExit, TerminalFrame,
};
use flight_trust::Role;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status, Streaming};

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

/// How long a peer has to send its `Attach` after the stream opens.
const ATTACH_FIRST_FRAME: Duration = Duration::from_secs(10);
/// After the UI's side ends, how long the node's side has to read what is still queued toward
/// it (the last keys, the goodbye) before the terminal is torn down regardless.
const NODE_DRAIN: Duration = Duration::from_secs(3);

fn origin(side: Side) -> Origin {
    match side {
        Side::Ui => Origin::Ui,
        Side::Node => Origin::Node,
    }
}

fn exit_frame(reason: ExitReasonCode) -> TerminalFrame {
    TerminalFrame {
        body: Some(terminal_body::Body::Exit(TerminalExit {
            reason: reason as i32,
            status: 0,
        })),
    }
}

fn close_frame() -> TerminalFrame {
    TerminalFrame {
        body: Some(terminal_body::Body::Close(TerminalClose {})),
    }
}

/// Every refusal looks the same to the caller.
fn refused() -> Status {
    Status::permission_denied("refused")
}

pub(crate) async fn terminal_stream(
    state: SharedState,
    request: Request<Streaming<TerminalFrame>>,
    side: Side,
) -> Result<Response<BoxStream<TerminalFrame>>, Status> {
    let peer = request
        .extensions()
        .get::<PeerIdentity>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("no authenticated peer"))?;
    let role = match side {
        Side::Node => Role::Node,
        Side::Ui => Role::Ui,
    };
    if !lock(&state).trust.is_authorized(&peer.0, role) {
        return Err(refused());
    }
    let mut inbound = request.into_inner();
    let first = tokio::time::timeout(ATTACH_FIRST_FRAME, inbound.message())
        .await
        .map_err(|_| refused())?
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    if first.validate_from(origin(side)).is_err() {
        return Err(refused());
    }
    let Some(terminal_body::Body::Attach(attach)) = first.body else {
        return Err(refused());
    };
    let (id, ends) = lock(&state)
        .attach_terminal(side, &attach.terminal_id, peer.0.as_str())
        .ok_or_else(refused)?;
    let Ends {
        forward,
        incoming,
        abort,
        peak,
        node_ended,
    } = ends;
    let (gone_tx, gone_rx) = oneshot::channel::<()>();
    let pump_state = state.clone();
    tokio::spawn(async move {
        let aborted = abort.clone();
        // Kept until this side is finished with: while it exists, the other side's stream
        // stays open, so what is still queued toward it can be read.
        let held = forward.clone();
        let reason = pump(&state, side, inbound, forward, abort, peak, gone_rx).await;
        match side {
            Side::Node => node_ended.notify_one(),
            // The UI is done (it said goodbye, or it is gone). Whatever it sent last is still
            // queued toward the node: tearing the terminal down now would discard it. The node
            // ends its side by itself when it reads the goodbye, so wait for that, bounded.
            Side::Ui if aborted.borrow().is_none() => {
                lock(&pump_state).terminal_closing(&id);
                let _ = tokio::time::timeout(NODE_DRAIN, node_ended.notified()).await;
            }
            Side::Ui => {}
        }
        lock(&pump_state).finish_terminal(id, side, reason);
        drop(held);
    });
    let out: BoxStream<TerminalFrame> = Box::pin(Watched {
        inner: ReceiverStream::new(incoming),
        _gone: gone_tx,
    });
    Ok(Response::new(out))
}

/// A response stream that tells the pump when it has been dropped, which is how a vanished
/// peer is noticed while the pump is busy waiting for room to send.
struct Watched {
    inner: ReceiverStream<TerminalFrame>,
    _gone: oneshot::Sender<()>,
}

impl Stream for Watched {
    type Item = Result<TerminalFrame, Status>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner)
            .poll_next(cx)
            .map(|item| item.map(Ok))
    }
}

/// Copy this side's inbound frames to the other side. Returns why it ended.
async fn pump(
    state: &SharedState,
    side: Side,
    mut inbound: Streaming<TerminalFrame>,
    forward: mpsc::Sender<TerminalFrame>,
    mut abort: watch::Receiver<Option<ExitReasonCode>>,
    peak: Arc<AtomicUsize>,
    mut gone: oneshot::Receiver<()>,
) -> ExitReasonCode {
    let stall = lock(state).terminal_stall();
    let vanished = match side {
        Side::Node => ExitReasonCode::NodeLost,
        Side::Ui => ExitReasonCode::ClosedByUi,
    };
    loop {
        tokio::select! {
            message = inbound.message() => {
                let frame = match message {
                    Ok(Some(frame)) => frame,
                    // The peer vanished without saying goodbye.
                    Ok(None) | Err(_) => return goodbye(side, &forward, stall).await,
                };
                if frame.validate_from(origin(side)).is_err()
                    || matches!(frame.body, Some(terminal_body::Body::Attach(_)))
                {
                    return goodbye(side, &forward, stall).await;
                }
                let last = frame.is_last();
                let used = forward.max_capacity().saturating_sub(forward.capacity());
                peak.fetch_max(used, Ordering::Relaxed);
                // Blocked here, this side stops reading, so its HTTP/2 window closes: that is
                // the backpressure. It ends if the terminal is aborted, this side's peer is
                // gone, or nothing moves for the whole stall limit.
                tokio::select! {
                    sent = tokio::time::timeout(stall, forward.send(frame)) => match sent {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => return ExitReasonCode::ClientExited,
                        Err(_) => {
                            if side == Side::Node {
                                let _ = forward.try_send(exit_frame(ExitReasonCode::Stalled));
                            }
                            return ExitReasonCode::Stalled;
                        }
                    },
                    _ = abort.changed() => return aborted(side, &abort, &forward, stall).await,
                    _ = &mut gone => return vanished,
                }
                if last {
                    return match side {
                        Side::Node => ExitReasonCode::ClientExited,
                        Side::Ui => ExitReasonCode::ClosedByUi,
                    };
                }
            }
            _ = abort.changed() => return aborted(side, &abort, &forward, stall).await,
            _ = &mut gone => return vanished,
        }
    }
}

/// The core ended this terminal: the UI is told why, the node just sees its stream end and
/// hangs up.
async fn aborted(
    side: Side,
    abort: &watch::Receiver<Option<ExitReasonCode>>,
    forward: &mpsc::Sender<TerminalFrame>,
    patience: Duration,
) -> ExitReasonCode {
    let reason = (*abort.borrow()).unwrap_or(ExitReasonCode::Shutdown);
    if side == Side::Node {
        let _ = tokio::time::timeout(
            patience.min(Duration::from_secs(1)),
            forward.send(exit_frame(reason)),
        )
        .await;
    }
    reason
}

/// This side's peer disappeared: tell the other end, in the vocabulary it understands.
async fn goodbye(
    side: Side,
    forward: &mpsc::Sender<TerminalFrame>,
    patience: Duration,
) -> ExitReasonCode {
    let (frame, reason) = match side {
        Side::Node => (
            exit_frame(ExitReasonCode::NodeLost),
            ExitReasonCode::NodeLost,
        ),
        Side::Ui => (close_frame(), ExitReasonCode::ClosedByUi),
    };
    let _ = tokio::time::timeout(patience.min(Duration::from_secs(1)), forward.send(frame)).await;
    reason
}
