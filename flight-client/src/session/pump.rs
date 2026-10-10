// SPDX-License-Identifier: MIT

use crate::session::FromRemote;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Why an attachment's output stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PumpEnd {
    /// The attachment ended or broke.
    Remote(FromRemote),
    /// The user's terminal stopped taking output.
    LocalGone,
}

/// Output of one attachment, tagged with the generation that produced it, so a late report from
/// an attachment that was already replaced is recognized and ignored.
#[derive(Debug)]
pub(super) struct Ended {
    pub(super) generation: u64,
    pub(super) end: PumpEnd,
}

/// The task copying one attachment's output to the user's terminal.
pub(super) struct Pump {
    task: JoinHandle<mpsc::Receiver<FromRemote>>,
    stop: oneshot::Sender<()>,
}

impl Pump {
    /// Stop reading the attachment's output at once.
    pub(super) fn abort(self) {
        self.task.abort();
    }

    /// Stop copying and give back what the attachment's output is read from, so another reader
    /// can take over where this one left off (output not yet copied is not lost, only what was in
    /// this reader's hands). `None` if the task could not be finished.
    pub(super) async fn take_back(self) -> Option<mpsc::Receiver<FromRemote>> {
        let _ = self.stop.send(());
        self.task.await.ok()
    }
}

/// Copy one attachment's output to the user's terminal until it ends or is stopped. `first` is
/// written before anything else (the terminal reset between two surfaces). Output is written
/// with backpressure: while the terminal is not taking it, nothing more is read from the stream.
pub(super) fn spawn(
    generation: u64,
    mut from_remote: mpsc::Receiver<FromRemote>,
    output: mpsc::Sender<Vec<u8>>,
    ended: mpsc::Sender<Ended>,
    first: Option<Vec<u8>>,
) -> Pump {
    let (stop, mut stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        if let Some(first) = first {
            tokio::select! {
                biased;
                _ = &mut stopped => return from_remote,
                sent = output.send(first) => if sent.is_err() {
                    let _ = ended
                        .send(Ended { generation, end: PumpEnd::LocalGone })
                        .await;
                    return from_remote;
                },
            }
        }
        let end = loop {
            let next = tokio::select! {
                biased;
                _ = &mut stopped => return from_remote,
                next = from_remote.recv() => next,
            };
            match next {
                Some(FromRemote::Data(bytes)) => {
                    tokio::select! {
                        biased;
                        _ = &mut stopped => return from_remote,
                        sent = output.send(bytes) => if sent.is_err() {
                            break PumpEnd::LocalGone;
                        },
                    }
                }
                Some(other) => break PumpEnd::Remote(other),
                None => break PumpEnd::Remote(FromRemote::Lost("the stream ended".to_owned())),
            }
        };
        let _ = ended.send(Ended { generation, end }).await;
        from_remote
    });
    Pump { task, stop }
}
