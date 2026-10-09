// SPDX-License-Identifier: MIT

use crate::collect::{Backend, CreateFailure};
use crate::snapshot::{PanePreview, PaneView, UiSnapshot};
use crate::{NewSessionRequest, NewSurfaceRequest};
use flight_state::PaneRef;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub enum Cmd {
    Refresh,
    Select(Option<PaneRef>),
    Switch(PaneView),
    Create(NewSessionRequest),
    CreateSurface(NewSurfaceRequest),
    Shutdown,
}

pub enum Msg {
    Snapshot(UiSnapshot),
    Preview(Option<PanePreview>),
    Switched(Result<(), String>),
    Created(NewSessionRequest, Result<(), CreateFailure>),
    SurfaceCreated(NewSurfaceRequest, Result<(), CreateFailure>),
}

pub struct Worker {
    pub tx: Sender<Cmd>,
    pub rx: Receiver<Msg>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Collection runs here, off the UI thread, so a slow or dead host can never freeze the
    /// interface: the loop keeps drawing and handling keys while a refresh is in flight.
    pub fn spawn(mut collector: Box<dyn Backend>, interval: Duration) -> Self {
        let (tx, cmd_rx) = channel::<Cmd>();
        let (msg_tx, rx) = channel::<Msg>();
        let handle = thread::spawn(move || {
            let mut target: Option<PaneRef> = None;
            if !refresh(&mut *collector, &target, &msg_tx) {
                return;
            }
            loop {
                let alive = match cmd_rx.recv_timeout(interval) {
                    Ok(Cmd::Refresh) | Err(RecvTimeoutError::Timeout) => {
                        refresh(&mut *collector, &target, &msg_tx)
                    }
                    Ok(Cmd::Select(p)) => {
                        target = p;
                        msg_tx
                            .send(Msg::Preview(target.as_ref().map(|t| collector.preview(t))))
                            .is_ok()
                    }
                    Ok(Cmd::Switch(p)) => {
                        let r = collector.switch_to(&p);
                        msg_tx.send(Msg::Switched(r)).is_ok()
                    }
                    Ok(Cmd::Create(request)) => {
                        let r = collector.create_session(&request);
                        // The new session shows up in the next snapshot: ask for it now.
                        msg_tx.send(Msg::Created(request, r)).is_ok()
                            && refresh(&mut *collector, &target, &msg_tx)
                    }
                    Ok(Cmd::CreateSurface(request)) => {
                        let r = collector.create_surface(&request);
                        msg_tx.send(Msg::SurfaceCreated(request, r)).is_ok()
                            && refresh(&mut *collector, &target, &msg_tx)
                    }
                    Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => false,
                };
                if !alive {
                    return;
                }
            }
        });
        Self {
            tx,
            rx,
            handle: Some(handle),
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Collect and send a snapshot plus the current preview. `false` once the UI is gone.
fn refresh(c: &mut dyn Backend, target: &Option<PaneRef>, tx: &Sender<Msg>) -> bool {
    let snapshot = c.snapshot(now_secs());
    let preview = target.as_ref().map(|t| c.preview(t));
    tx.send(Msg::Snapshot(snapshot)).is_ok() && tx.send(Msg::Preview(preview)).is_ok()
}
