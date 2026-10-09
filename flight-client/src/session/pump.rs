// SPDX-License-Identifier: MIT

use crate::session::FromRemote;
use tokio::sync::mpsc;
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

/// Copy one attachment's output to the user's terminal until it ends. `first` is written
/// before anything else (the terminal reset between two surfaces). Output is written with
/// backpressure: while the terminal is not taking it, nothing more is read from the stream.
pub(super) fn spawn(
    generation: u64,
    mut from_remote: mpsc::Receiver<FromRemote>,
    output: mpsc::Sender<Vec<u8>>,
    ended: mpsc::Sender<Ended>,
    first: Option<Vec<u8>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Some(first) = first {
            if output.send(first).await.is_err() {
                let _ = ended
                    .send(Ended {
                        generation,
                        end: PumpEnd::LocalGone,
                    })
                    .await;
                return;
            }
        }
        let end = loop {
            match from_remote.recv().await {
                Some(FromRemote::Data(bytes)) => {
                    if output.send(bytes).await.is_err() {
                        break PumpEnd::LocalGone;
                    }
                }
                Some(other) => break PumpEnd::Remote(other),
                None => break PumpEnd::Remote(FromRemote::Lost("the stream ended".to_owned())),
            }
        };
        let _ = ended.send(Ended { generation, end }).await;
    })
}
