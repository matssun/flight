// SPDX-License-Identifier: MIT

use crate::snapshot_view::ui_snapshot;
use crate::switch::{Handoff, HandoffSlot, Presented, RemoteOps, SwitchTarget, Switcher, TmuxEnv};
use crate::terminal::terminal_request_shape;
use flight_proto::{
    command_kind as ck, response_result, ui_event_body, ui_request_body, Command, ErrorKindCode,
    FleetImage, PaneRefMsg, Request, Step, Subscribe, UiEvent, UiRequest,
};
use flight_state::PaneRef;
use flight_transport::UiClient;
use flight_trust::{ConnectionConfig, Fingerprint, Identity, TrustError};
use flight_ui::{Backend, PanePreview, PaneView, UiSnapshot};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, watch};

/// Lines of a pane requested for the preview.
const PREVIEW_LINES: u32 = 200;
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(5);
/// The orchestrator gives up on a node after 10 s; wait a little longer for its answer.
const REVEAL_TIMEOUT: Duration = Duration::from_secs(12);
const RECONNECT_MIN: Duration = Duration::from_millis(250);
const RECONNECT_MAX: Duration = Duration::from_secs(5);

/// Where the dashboard finds its orchestrator: an endpoint and a pinned identity, nothing more.
/// It is the same whether the orchestrator is on this machine, on the LAN, or hosted.
#[derive(Clone)]
pub struct ClientConfig {
    pub address: String,
    pub identity: Arc<Identity>,
    pub orchestrator: Fingerprint,
}

impl ClientConfig {
    /// Load a joined UI's identity and connection settings from its directory.
    pub fn load(role_dir: &Path) -> Result<Self, TrustError> {
        let config = ConnectionConfig::load(&flight_transport::config_path(role_dir))?;
        Ok(Self {
            address: config.address.clone(),
            orchestrator: config.orchestrator()?,
            identity: Arc::new(Identity::load(&flight_transport::identity_dir(role_dir))?),
        })
    }
}

#[derive(Default)]
struct LinkState {
    image: FleetImage,
    connected: bool,
    last_error: Option<String>,
}

type Shared = Arc<Mutex<LinkState>>;

fn lock(state: &Shared) -> MutexGuard<'_, LinkState> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

/// What a control command's answer carried.
enum Answer {
    Done,
    Text(String),
    Terminal(Vec<u8>),
}

/// One control command to send over the link, and where its answer goes.
struct ControlRequest {
    kind: ck::Kind,
    reply: oneshot::Sender<Result<Answer, String>>,
}

/// The dashboard's backend over an orchestrator. A background task keeps the link up and a
/// [`FleetImage`] current; `snapshot` just renders that image, so refreshing never waits on
/// the network.
pub struct OrchestratedBackend {
    runtime: Runtime,
    state: Shared,
    requests: mpsc::Sender<ControlRequest>,
    stop: watch::Sender<bool>,
    switcher: Switcher,
    handoff: HandoffSlot,
}

impl OrchestratedBackend {
    pub fn start(config: ClientConfig) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let state: Shared = Arc::default();
        let (requests, rx) = mpsc::channel(16);
        let (stop, stop_rx) = watch::channel(false);
        runtime.spawn(link_loop(config, state.clone(), rx, stop_rx));
        Ok(Self {
            runtime,
            state,
            requests,
            stop,
            switcher: Switcher::default(),
            handoff: HandoffSlot::default(),
        })
    }

    /// How Enter finds and shows a pane: which node is this machine, and where ssh goes.
    /// Without it every pane is remote and none has a destination, so Enter explains why not.
    pub fn set_switching(&mut self, switcher: Switcher) {
        self.switcher = switcher;
    }

    /// Where an attach waits for the terminal after the dashboard exits.
    pub fn handoff(&self) -> HandoffSlot {
        self.handoff.clone()
    }

    /// Whether the stream to the orchestrator is up right now.
    pub fn connected(&self) -> bool {
        lock(&self.state).connected
    }
}

impl Drop for OrchestratedBackend {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

impl Backend for OrchestratedBackend {
    fn snapshot(&mut self, now: u64) -> UiSnapshot {
        let state = lock(&self.state);
        ui_snapshot(
            &state.image,
            state.connected,
            state.last_error.as_deref(),
            now,
        )
    }

    fn preview(&mut self, pane: &PaneRef) -> PanePreview {
        let kind = ck::Kind::GetPreview(ck::GetPreview {
            pane_ref: Some(PaneRefMsg::from(pane)),
            lines: PREVIEW_LINES,
        });
        let content = self
            .runtime
            .block_on(call(&self.requests, kind, PREVIEW_TIMEOUT))
            .and_then(|answer| match answer {
                Answer::Text(text) => Ok(text),
                _ => Err("unexpected response".to_owned()),
            });
        PanePreview {
            pane: pane.clone(),
            content: content.map(|text| text.lines().map(str::to_owned).collect()),
        }
    }

    fn switch_to(&mut self, pane: &PaneView) -> Result<(), String> {
        let target = SwitchTarget::from(pane);
        let mut remote = LinkOps {
            runtime: &self.runtime,
            requests: &self.requests,
        };
        match self
            .switcher
            .switch(&target, &TmuxEnv::from_process(), &mut remote)
        {
            Ok(Presented::ClientMoved) => Ok(()),
            Ok(Presented::Attach(command)) => {
                self.handoff.put(Handoff::Attach(command));
                Ok(())
            }
            Ok(Presented::Terminal(id)) => {
                self.handoff.put(Handoff::Terminal(id));
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// A switch's requests to the orchestrator, over the dashboard's own link.
struct LinkOps<'a> {
    runtime: &'a Runtime,
    requests: &'a mpsc::Sender<ControlRequest>,
}

impl RemoteOps for LinkOps<'_> {
    fn reveal(&mut self, pane: &PaneRef, pid: u32) -> Result<(), String> {
        let kind = ck::Kind::RevealPane(ck::RevealPane {
            pane_ref: Some(PaneRefMsg::from(pane)),
            expected_pid: pid,
        });
        self.runtime
            .block_on(call(self.requests, kind, REVEAL_TIMEOUT))
            .map(drop)
    }

    fn open_terminal(&mut self, pane: &PaneRef, pid: u32) -> Result<Vec<u8>, String> {
        let (cols, rows, term) = terminal_request_shape();
        let kind = ck::Kind::OpenTerminal(ck::OpenTerminal {
            pane_ref: Some(PaneRefMsg::from(pane)),
            expected_pid: pid,
            cols: u32::from(cols),
            rows: u32::from(rows),
            term,
            terminal_id: Vec::new(),
        });
        self.runtime
            .block_on(call(self.requests, kind, REVEAL_TIMEOUT))
            .and_then(|answer| match answer {
                Answer::Terminal(id) => Ok(id),
                _ => Err("the orchestrator answered without a terminal".to_owned()),
            })
    }
}

/// Send one command and wait for its answer, bounded.
async fn call(
    requests: &mpsc::Sender<ControlRequest>,
    kind: ck::Kind,
    timeout: Duration,
) -> Result<Answer, String> {
    let (reply, answer) = oneshot::channel();
    if requests.send(ControlRequest { kind, reply }).await.is_err() {
        return Err("not connected to the orchestrator".to_owned());
    }
    match tokio::time::timeout(timeout, answer).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("connection to the orchestrator was lost".to_owned()),
        Err(_) => Err("no confirmation from the orchestrator in time".to_owned()),
    }
}

fn subscribe() -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Subscribe(Subscribe {})),
    }
}

fn command_request(id: u64, kind: ck::Kind) -> UiRequest {
    UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: id,
            command: Some(Command { kind: Some(kind) }),
        })),
    }
}

/// Keep a link to the orchestrator up, re-dialling with backoff, until stopped.
async fn link_loop(
    config: ClientConfig,
    state: Shared,
    mut requests: mpsc::Receiver<ControlRequest>,
    mut stop: watch::Receiver<bool>,
) {
    let mut delay = RECONNECT_MIN;
    loop {
        let session = serve_link(&config, &state, &mut requests);
        let outcome = tokio::select! {
            _ = stop.changed() => return,
            outcome = session => outcome,
        };
        {
            let mut s = lock(&state);
            s.connected = false;
            s.image.disconnected();
            s.last_error = Some(outcome);
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(delay) => {}
        }
        delay = (delay * 2).min(RECONNECT_MAX);
    }
}

/// One connection's lifetime; returns why it ended.
async fn serve_link(
    config: &ClientConfig,
    state: &Shared,
    requests: &mut mpsc::Receiver<ControlRequest>,
) -> String {
    let mut client =
        match UiClient::connect(&config.address, &config.identity, &config.orchestrator).await {
            Ok(c) => c,
            Err(e) => return e.to_string(),
        };
    if client.send(subscribe()).is_err() {
        return "cannot subscribe".to_owned();
    }
    let mut next_id = 0u64;
    let mut waiting: HashMap<u64, oneshot::Sender<Result<Answer, String>>> = HashMap::new();
    loop {
        tokio::select! {
            event = client.next_event() => match event {
                Ok(Some(event)) => {
                    if apply(state, &event, &mut waiting) == Step::Resync && client.send(subscribe()).is_err() {
                        return "cannot resubscribe".to_owned();
                    }
                }
                Ok(None) => return "the orchestrator closed the connection".to_owned(),
                Err(e) => return e.to_string(),
            },
            Some(req) = requests.recv() => {
                next_id += 1;
                if client.send(command_request(next_id, req.kind)).is_ok() {
                    waiting.insert(next_id, req.reply);
                } else {
                    let _ = req.reply.send(Err("too many requests in flight".to_owned()));
                }
            }
        }
    }
}

fn apply(
    state: &Shared,
    event: &UiEvent,
    waiting: &mut HashMap<u64, oneshot::Sender<Result<Answer, String>>>,
) -> Step {
    if let Some(ui_event_body::Body::Response(r)) = event.body.as_ref() {
        if let Some(reply) = waiting.remove(&r.request_id) {
            let _ = reply.send(match r.result.as_ref() {
                Some(response_result::Result::Preview(p)) => Ok(Answer::Text(p.text.clone())),
                Some(response_result::Result::Terminal(t)) => {
                    Ok(Answer::Terminal(t.terminal_id.clone()))
                }
                Some(response_result::Result::Done(_)) => Ok(Answer::Done),
                Some(response_result::Result::Error(e)) => Err(describe(e)),
                _ => Err("unexpected response".to_owned()),
            });
        }
        return Step::Apply;
    }
    let mut s = lock(state);
    let step = s.image.apply(event);
    if step == Step::Apply && matches!(event.body, Some(ui_event_body::Body::Snapshot(_))) {
        s.connected = true;
        s.last_error = None;
    }
    step
}

/// What the user is told about a node's refusal. A changed pane is not an error to retry but
/// a prompt to look at the dashboard again.
fn describe(e: &flight_proto::ErrorInfo) -> String {
    if e.kind == ErrorKindCode::PaneChanged as i32 {
        "the pane changed since it was listed; refresh".to_owned()
    } else {
        e.message.clone()
    }
}
