// SPDX-License-Identifier: MIT

use crate::session::{Binding, OpenFailure};
use crate::snapshot_view::ui_snapshot;
use crate::switch::{
    Handoff, HandoffSlot, Presented, RemoteOps, ShownSurface, SwitchTarget, Switcher, TmuxEnv,
};
use crate::terminal::terminal_request_shape;
use flight_proto::{
    command_kind as ck, response_result, ui_event_body, ui_request_body, Command, ErrorKindCode,
    FleetImage, PaneRefMsg, ProgramCode, Request, SavedActionCode, Step, Subscribe,
    SurfaceKindCode, TerminalLease, UiEvent, UiRequest,
};
use flight_state::PaneRef;
use flight_transport::{TerminalConnector, UiClient};
use flight_trust::{ConnectionConfig, Fingerprint, Identity, TrustError};
use flight_ui::{
    Backend, CreateFailure, NewSessionRequest, NewSurfaceRequest, PanePreview, PaneView, Program,
    SavedActionKind, SavedActionRequest, SurfaceChoice, UiSnapshot, WorkspaceKey,
};
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
/// Creating a session waits for the node's launch check as well.
const CREATE_TIMEOUT: Duration = Duration::from_secs(12);
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

/// Why a command did not succeed: the refusal's type when a node or the orchestrator gave one,
/// and what to tell the user either way.
struct Failure {
    kind: Option<ErrorKindCode>,
    message: String,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            kind: None,
            message,
        }
    }
}

/// One control command to send over the link, and where its answer goes.
struct ControlRequest {
    kind: ck::Kind,
    reply: oneshot::Sender<Result<Answer, Failure>>,
}

/// What the link is asked to send.
enum LinkRequest {
    Command(ControlRequest),
    /// The presentation of this terminal is alive (ADR-004).
    Lease(Vec<u8>),
}

/// The dashboard's backend over an orchestrator. A background task keeps the link up and a
/// [`FleetImage`] current; `snapshot` just renders that image, so refreshing never waits on
/// the network.
///
/// Cloning gives another handle to the same link: the connection, its runtime and the image
/// outlive any one dashboard run, so showing a terminal and coming back does not rebuild them.
/// The link stops when the last handle is dropped.
#[derive(Clone)]
pub struct OrchestratedBackend {
    inner: Arc<Link>,
    switcher: Switcher,
    handoff: HandoffSlot,
}

/// The link itself: shared by every handle.
struct Link {
    /// The connection terminal streams ride, kept between terminals.
    terminals: Arc<TerminalConnector>,
    runtime: Runtime,
    state: Shared,
    requests: mpsc::Sender<LinkRequest>,
    stop: watch::Sender<bool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

impl OrchestratedBackend {
    pub fn start(config: ClientConfig) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let terminals = Arc::new(TerminalConnector::new(
            &config.address,
            config.identity.clone(),
            config.orchestrator.clone(),
        ));
        let state: Shared = Arc::default();
        let (requests, rx) = mpsc::channel(16);
        let (stop, stop_rx) = watch::channel(false);
        runtime.spawn(link_loop(config, state.clone(), rx, stop_rx));
        Ok(Self {
            inner: Arc::new(Link {
                terminals,
                runtime,
                state,
                requests,
                stop,
            }),
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
        lock(&self.inner.state).connected
    }

    /// The runtime the link runs on. Whatever works on the link's behalf (a terminal session)
    /// runs here too, so there is one set of worker threads and one place to stop them.
    pub fn runtime(&self) -> &Runtime {
        &self.inner.runtime
    }

    /// The connection for terminal streams: one is dialed on first use and kept, so showing a
    /// surface is one more stream on a connection that is already up.
    pub(crate) fn terminals(&self) -> &TerminalConnector {
        &self.inner.terminals
    }

    /// The fleet as the link last saw it, for finding the surface to show.
    pub(crate) fn current_snapshot(&self) -> UiSnapshot {
        let state = lock(&self.inner.state);
        ui_snapshot(
            &state.image,
            state.connected,
            state.last_error.as_deref(),
            0,
        )
    }

    /// Ask the orchestrator for a terminal onto `pane`, guarded by the process the caller saw.
    pub(crate) async fn request_terminal(
        &self,
        pane: &PaneRef,
        pid: u32,
        (cols, rows, term): (u16, u16, String),
    ) -> Result<Vec<u8>, OpenFailure> {
        // A request queued behind a link that is down would wait out the whole timeout for an
        // answer that cannot come; say so at once, and let the caller try again.
        if !self.connected() {
            return Err(OpenFailure::Unavailable(
                "not connected to the orchestrator".to_owned(),
            ));
        }
        let kind = ck::Kind::OpenTerminal(ck::OpenTerminal {
            pane_ref: Some(PaneRefMsg::from(pane)),
            expected_pid: pid,
            cols: u32::from(cols),
            rows: u32::from(rows),
            term,
            terminal_id: Vec::new(),
        });
        match call(&self.inner.requests, kind, REVEAL_TIMEOUT).await {
            Ok(Answer::Terminal(id)) => Ok(id),
            Ok(_) => Err(OpenFailure::Refused(
                "the orchestrator answered without a terminal".to_owned(),
            )),
            Err(f) => Err(match f.kind {
                // Nothing is wrong with the request; the node, the link or a limit is not
                // available right now.
                None | Some(ErrorKindCode::NodeUnreachable | ErrorKindCode::Busy) => {
                    OpenFailure::Unavailable(f.message)
                }
                Some(_) => OpenFailure::Refused(f.message),
            }),
        }
    }

    /// Say a terminal's presentation is alive. Fails only when the link has stopped; while it is
    /// reconnecting the lease lifetime (several periods) covers the gap.
    pub(crate) fn send_lease(&self, terminal_id: &[u8]) -> Result<(), String> {
        match self
            .inner
            .requests
            .try_send(LinkRequest::Lease(terminal_id.to_vec()))
        {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err("the link to the orchestrator has stopped".to_owned())
            }
        }
    }
}

impl Backend for OrchestratedBackend {
    fn snapshot(&mut self, now: u64) -> UiSnapshot {
        let state = lock(&self.inner.state);
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
            .inner
            .runtime
            .block_on(call(&self.inner.requests, kind, PREVIEW_TIMEOUT))
            .map_err(|f| f.message)
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
            runtime: &self.inner.runtime,
            requests: &self.inner.requests,
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
                self.handoff.put(Handoff::Terminal {
                    binding: Binding {
                        pane: pane.pane_ref.clone(),
                        pid: pane.pid,
                    },
                    id,
                    shown: ShownSurface {
                        workspace: WorkspaceKey {
                            host: pane.pane_ref.host.clone(),
                            workspace: pane.workspace.clone(),
                        },
                        choice: if pane.kind.is_agent() {
                            SurfaceChoice::Agent
                        } else {
                            SurfaceChoice::Shell
                        },
                    },
                });
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn create_surface(&mut self, request: &NewSurfaceRequest) -> Result<(), CreateFailure> {
        // The workspace id and the kind, and nothing else: the orchestrator finds the host and
        // the node finds the directory.
        let kind = ck::Kind::CreateSurface(ck::CreateSurface {
            workspace_id: request.workspace.as_str().to_owned(),
            kind: match request.kind {
                SurfaceChoice::Shell => SurfaceKindCode::Shell,
                SurfaceChoice::Agent => SurfaceKindCode::Agent,
            } as i32,
        });
        self.inner
            .runtime
            .block_on(call(&self.inner.requests, kind, CREATE_TIMEOUT))
            .map(drop)
            .map_err(create_failure)
    }

    fn saved_action(&mut self, request: &SavedActionRequest) -> Result<(), CreateFailure> {
        let (action, root) = match &request.action {
            SavedActionKind::Retry => (SavedActionCode::Retry, String::new()),
            SavedActionKind::Remove => (SavedActionCode::Remove, String::new()),
            SavedActionKind::Restore => (SavedActionCode::Restore, String::new()),
            SavedActionKind::RestoreFresh => (SavedActionCode::RestoreFresh, String::new()),
            SavedActionKind::AcceptRoot => (SavedActionCode::AcceptRoot, String::new()),
            SavedActionKind::SetRoot(path) => (SavedActionCode::SetRoot, path.clone()),
            SavedActionKind::Trust => (SavedActionCode::Trust, String::new()),
        };
        let kind = ck::Kind::SavedAction(ck::SavedAction {
            host: request.host.as_str().to_owned(),
            config_key: request.config_key.clone(),
            action: action as i32,
            root,
        });
        self.inner
            .runtime
            .block_on(call(&self.inner.requests, kind, CREATE_TIMEOUT))
            .map(drop)
            .map_err(create_failure)
    }

    fn create_session(&mut self, request: &NewSessionRequest) -> Result<(), CreateFailure> {
        let kind = ck::Kind::CreateSession(ck::CreateSession {
            host: request.host.as_str().to_owned(),
            name: request.name.clone(),
            dir: request.dir.clone(),
            program: match request.program {
                Program::Claude => ProgramCode::Claude,
                Program::ClaudeSkipPermissions => ProgramCode::ClaudeSkipPermissions,
                Program::Shell => ProgramCode::Shell,
            } as i32,
        });
        self.inner
            .runtime
            .block_on(call(&self.inner.requests, kind, CREATE_TIMEOUT))
            .map(drop)
            .map_err(create_failure)
    }
}

/// A refusal of a create request, as the dashboard tells them apart.
fn create_failure(f: Failure) -> CreateFailure {
    match f.kind {
        Some(ErrorKindCode::AlreadyExists) => CreateFailure::AlreadyExists,
        Some(ErrorKindCode::InvalidDirectory) => CreateFailure::NoSuchDirectory(f.message),
        Some(ErrorKindCode::ProgramUnavailable) => CreateFailure::ProgramUnavailable(f.message),
        Some(ErrorKindCode::NodeUnreachable) => CreateFailure::Unreachable,
        Some(ErrorKindCode::UnknownWorkspace) => CreateFailure::UnknownWorkspace,
        // An orchestrator that predates a command cannot even decode it, and says so in its
        // own words; the user should hear what to do, not that.
        Some(ErrorKindCode::InvalidRequest) if f.message.contains("command.kind") => {
            CreateFailure::Other(
                "The orchestrator is too old for this. Update Flight on it.".to_owned(),
            )
        }
        Some(ErrorKindCode::Unsupported) => {
            CreateFailure::Other("That node is too old for this. Update Flight on it.".to_owned())
        }
        _ => CreateFailure::Other(f.message),
    }
}

/// A switch's requests to the orchestrator, over the dashboard's own link.
struct LinkOps<'a> {
    runtime: &'a Runtime,
    requests: &'a mpsc::Sender<LinkRequest>,
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
            .map_err(|f| f.message)
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
            .map_err(|f| f.message)
            .and_then(|answer| match answer {
                Answer::Terminal(id) => Ok(id),
                _ => Err("the orchestrator answered without a terminal".to_owned()),
            })
    }
}

/// Send one command and wait for its answer, bounded.
async fn call(
    requests: &mpsc::Sender<LinkRequest>,
    kind: ck::Kind,
    timeout: Duration,
) -> Result<Answer, Failure> {
    let (reply, answer) = oneshot::channel();
    if requests
        .send(LinkRequest::Command(ControlRequest { kind, reply }))
        .await
        .is_err()
    {
        return Err("not connected to the orchestrator".to_owned().into());
    }
    match tokio::time::timeout(timeout, answer).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("connection to the orchestrator was lost".to_owned().into()),
        Err(_) => Err("no confirmation from the orchestrator in time"
            .to_owned()
            .into()),
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
    mut requests: mpsc::Receiver<LinkRequest>,
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
    requests: &mut mpsc::Receiver<LinkRequest>,
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
    let mut waiting: HashMap<u64, oneshot::Sender<Result<Answer, Failure>>> = HashMap::new();
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
            Some(request) = requests.recv() => match request {
                LinkRequest::Command(req) => {
                    next_id += 1;
                    if client.send(command_request(next_id, req.kind)).is_ok() {
                        waiting.insert(next_id, req.reply);
                    } else {
                        let _ = req
                            .reply
                            .send(Err("too many requests in flight".to_owned().into()));
                    }
                }
                LinkRequest::Lease(terminal_id) => {
                    // Best effort: a lease that is not sent is covered by the next one, and the
                    // orchestrator's lifetime for a terminal is several periods.
                    let _ = client.send(UiRequest {
                        body: Some(ui_request_body::Body::TerminalLease(TerminalLease { terminal_id })),
                    });
                }
            },
        }
    }
}

fn apply(
    state: &Shared,
    event: &UiEvent,
    waiting: &mut HashMap<u64, oneshot::Sender<Result<Answer, Failure>>>,
) -> Step {
    if let Some(ui_event_body::Body::Response(r)) = event.body.as_ref() {
        if let Some(reply) = waiting.remove(&r.request_id) {
            let _ = reply.send(match r.result.as_ref() {
                Some(response_result::Result::Preview(p)) => Ok(Answer::Text(p.text.clone())),
                Some(response_result::Result::Terminal(t)) => {
                    Ok(Answer::Terminal(t.terminal_id.clone()))
                }
                Some(response_result::Result::Done(_)) => Ok(Answer::Done),
                Some(response_result::Result::Error(e)) => Err(Failure {
                    kind: ErrorKindCode::try_from(e.kind).ok(),
                    message: describe(e),
                }),
                _ => Err("unexpected response".to_owned().into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(kind: ErrorKindCode, message: &str) -> Failure {
        Failure {
            kind: Some(kind),
            message: message.to_owned(),
        }
    }

    #[test]
    fn an_orchestrator_that_cannot_decode_the_command_is_called_too_old() {
        let f = create_failure(failure(
            ErrorKindCode::InvalidRequest,
            "missing command.kind",
        ));
        assert_eq!(
            f,
            CreateFailure::Other(
                "The orchestrator is too old for this. Update Flight on it.".into()
            )
        );
    }

    #[test]
    fn typed_refusals_stay_typed() {
        assert_eq!(
            create_failure(failure(ErrorKindCode::UnknownWorkspace, "x")),
            CreateFailure::UnknownWorkspace
        );
        assert_eq!(
            create_failure(failure(ErrorKindCode::NodeUnreachable, "x")),
            CreateFailure::Unreachable
        );
        assert_eq!(
            create_failure(failure(ErrorKindCode::AlreadyExists, "x")),
            CreateFailure::AlreadyExists
        );
    }
}
