// SPDX-License-Identifier: MIT

use crate::connector::{connect_with, DEFAULT_DIAL_TIMEOUT};
use crate::outbox::Outbox;
use crate::paths::NODE_CONNECT;
use crate::shared::{now, OUTBOX_CAPACITY};
use crate::TransportError;
use flight_node::{error_frame, Control, ControlJob, NodeSession, Round};
use flight_proto::{ErrorKindCode, NodeFrame, OrchestratorFrame};
use flight_trust::{Fingerprint, Identity};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{watch, Semaphore};
use tokio_stream::wrappers::ReceiverStream;
use tonic::client::Grpc;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::Request;
use tonic_prost::ProstCodec;

/// Control operations (tmux capture, kill, ...) running at once.
const MAX_CONCURRENT_JOBS: usize = 4;
/// A control operation that has not finished by then is answered as failed.
const JOB_TIMEOUT: Duration = Duration::from_secs(10);
/// Outbox class of heartbeats: at most one waits to be sent.
const HEARTBEAT: u8 = 0;

/// Why [`NodeLink::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEnd {
    /// Stopped on request.
    Stopped,
    /// This process could not route to the orchestrator for the whole configured window
    /// (immediate "no route"/"network unreachable" errors only: an orchestrator that is down,
    /// slow or refusing never counts). On macOS a long-running process has been seen to stay
    /// in this state after a network interface bounce while a fresh process connects at once;
    /// the caller should exit so that a supervisor restarts it.
    ProcessNetworkUnhealthy,
}

/// Tracks an unbroken run of local "unreachable" failures. Pure, so it is tested with
/// synthetic time.
#[derive(Debug, Clone)]
pub(crate) struct UnreachableWatch {
    limit: Duration,
    since: Option<std::time::Instant>,
}

impl UnreachableWatch {
    pub(crate) fn new(limit: Duration) -> Self {
        Self { limit, since: None }
    }

    /// How long the current unbroken run of failures has lasted at `now`.
    pub(crate) fn window(&self, now: std::time::Instant) -> Duration {
        self.since.map_or(Duration::ZERO, |s| now.duration_since(s))
    }

    /// Record the outcome of an attempt at `now`; true when the window has been exceeded.
    pub(crate) fn record(&mut self, unreachable: bool, now: std::time::Instant) -> bool {
        if !unreachable {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.duration_since(since) >= self.limit
    }
}

/// `limit` plus up to a quarter, so a fleet of nodes does not restart in lock step.
fn jittered(limit: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    limit + limit / 4 * (nanos % 1000) as u32 / 1000
}

/// Receives one-line notes about the link (connected, why it ended), for the operator.
pub type LinkLog = Arc<dyn Fn(String) + Send + Sync>;

pub struct NodeLinkConfig {
    /// `host:port` of the orchestrator.
    pub address: String,
    pub identity: Arc<Identity>,
    /// The orchestrator's pinned identity.
    pub orchestrator: Fingerprint,
    /// The tmux servers this node observes (advertised in the hello).
    pub servers: Vec<String>,
    pub heartbeat_interval: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
}

type Out = Arc<Mutex<Option<Arc<Outbox<NodeFrame>>>>>;

/// Clears the current outbox however `run_once` ends, including when its future is dropped
/// mid-flight: a lingering sender would keep the stream open after the link was stopped.
struct ClearOnDrop(Out);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        if let Some(outbox) = guard(&self.0).take() {
            outbox.close();
        }
    }
}

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A node's connection to the orchestrator: one long-lived outbound bidirectional stream,
/// re-dialled with backoff. The protocol lives in the `NodeSession`; the session lock covers
/// state transitions only, never external I/O (control work runs on blocking threads).
pub struct NodeLink {
    cfg: NodeLinkConfig,
    session: Arc<Mutex<NodeSession>>,
    control: Arc<dyn Control>,
    jobs: Arc<Semaphore>,
    out: Out,
    log: Option<LinkLog>,
    repeat_report: Duration,
    dial_timeout: Duration,
    unreachable_limit: Option<Duration>,
}

impl NodeLink {
    pub fn new(cfg: NodeLinkConfig, session: NodeSession, control: Arc<dyn Control>) -> Self {
        Self {
            cfg,
            session: Arc::new(Mutex::new(session)),
            control,
            jobs: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
            out: Arc::default(),
            log: None,
            repeat_report: Duration::from_secs(30),
            dial_timeout: DEFAULT_DIAL_TIMEOUT,
            unreachable_limit: None,
        }
    }

    /// Bound on each dialling step (TCP connect, TLS handshake); default 5 s.
    pub fn with_dial_timeout(mut self, timeout: Duration) -> Self {
        self.dial_timeout = timeout;
        self
    }

    /// Make [`run`](Self::run) return [`LinkEnd::ProcessNetworkUnhealthy`] after this long
    /// of nothing but immediate "no route" failures (jittered by up to 25%). Off by default.
    pub fn with_unreachable_limit(mut self, limit: Duration) -> Self {
        self.unreachable_limit = Some(limit);
        self
    }

    /// How often a failure that keeps repeating is reported again (default 30 s).
    pub fn with_repeat_report(mut self, every: Duration) -> Self {
        self.repeat_report = every;
        self
    }

    /// Report link transitions to `log`. Repeated identical failures are reported once.
    pub fn with_log(mut self, log: LinkLog) -> Self {
        self.log = Some(log);
        self
    }

    fn say(&self, line: String) {
        if let Some(log) = &self.log {
            log(line);
        }
    }

    /// Fold observations into the node's state and stream the resulting deltas, if connected.
    /// Deltas are disposable: if the peer falls behind they are discarded and a snapshot is
    /// sent instead. The session lock is held while queueing, so frames leave in order.
    pub fn observe(&self, rounds: Vec<Round>) {
        let mut session = guard(&self.session);
        let frames = session.observe(rounds);
        if let Some(outbox) = guard(&self.out).as_ref() {
            for f in frames {
                let _ = outbox.push_delta(f);
            }
        }
    }

    /// Read access to the session, for inspection.
    pub fn with_session<T>(&self, f: impl FnOnce(&NodeSession) -> T) -> T {
        f(&guard(&self.session))
    }

    /// Keep connected until `stop` flips, re-dialling with exponential backoff.
    pub async fn run(&self, mut stop: watch::Receiver<bool>) -> LinkEnd {
        let mut delay = self.cfg.reconnect_min;
        let mut watch = self
            .unreachable_limit
            .map(|l| UnreachableWatch::new(jittered(l)));
        let mut last_failure = String::new();
        let (mut attempts, mut failing_since, mut last_report) =
            (0u64, std::time::Instant::now(), std::time::Instant::now());
        loop {
            let started = std::time::Instant::now();
            let outcome = tokio::select! {
                _ = stop.changed() => return LinkEnd::Stopped,
                outcome = self.run_once() => outcome,
            };
            let unreachable = matches!(&outcome, Err(e) if e.is_unreachable());
            if let Some(w) = watch.as_mut() {
                let now = std::time::Instant::now();
                if w.record(unreachable, now) {
                    self.say(format!(
                        "this process has had no route to {} for {:.0?}; the network is probably fine but this process cannot use it, so it is exiting to be restarted",
                        self.cfg.address,
                        w.window(now)
                    ));
                    return LinkEnd::ProcessNetworkUnhealthy;
                }
            }
            match outcome {
                Ok(()) => {
                    last_failure.clear();
                    attempts = 0;
                    self.say(format!(
                        "link ended after {:.0?}: the orchestrator closed the connection",
                        started.elapsed()
                    ));
                }
                Err(e) => {
                    let why = e.to_string();
                    attempts += 1;
                    if why != last_failure {
                        self.say(format!(
                            "link down after {:.0?}: {why}; retrying",
                            started.elapsed()
                        ));
                        last_failure = why;
                        attempts = 1;
                        failing_since = std::time::Instant::now();
                        last_report = failing_since;
                    } else if last_report.elapsed() >= self.repeat_report {
                        self.say(format!(
                            "link still down: {attempts} attempts over {:.0?}: {last_failure}",
                            failing_since.elapsed()
                        ));
                        last_report = std::time::Instant::now();
                    }
                }
            }
            if started.elapsed() > self.cfg.reconnect_max {
                delay = self.cfg.reconnect_min;
            }
            tokio::select! {
                _ = stop.changed() => return LinkEnd::Stopped,
                _ = tokio::time::sleep(delay) => {}
            }
            delay = (delay * 2).min(self.cfg.reconnect_max);
        }
    }

    /// One connection: dial, handshake, serve until the stream ends.
    pub async fn run_once(&self) -> Result<(), TransportError> {
        let channel = connect_with(
            &self.cfg.address,
            &self.cfg.identity,
            &self.cfg.orchestrator,
            self.dial_timeout,
        )
        .await?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|e| TransportError::Connect(e.to_string()))?;
        let outbox = Arc::new(Outbox::new(OUTBOX_CAPACITY));
        {
            let mut session = guard(&self.session);
            let _ = outbox.push_reliable(session.connect(self.cfg.servers.clone()));
            *guard(&self.out) = Some(outbox.clone());
        }
        let _clear = ClearOnDrop(self.out.clone());
        let outbound = self.outbound(outbox.clone());
        self.serve_stream(&mut grpc, outbound, &outbox).await
    }

    /// The outbox as a gRPC request stream. When deltas were discarded, a fresh snapshot is
    /// queued under the session lock (which also orders it against new deltas).
    fn outbound(&self, outbox: Arc<Outbox<NodeFrame>>) -> ReceiverStream<NodeFrame> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let session = self.session.clone();
        let for_resync = outbox.clone();
        tokio::spawn(async move {
            let resync = move || {
                let mut session = guard(&session);
                let _ = for_resync.push_reliable(session.snapshot_frame());
                for_resync.clear_overflow();
            };
            while let Some(frame) = outbox.next(&resync).await {
                if tx.send(frame).await.is_err() {
                    outbox.close();
                    break;
                }
            }
        });
        ReceiverStream::new(rx)
    }

    async fn serve_stream(
        &self,
        grpc: &mut Grpc<tonic::transport::Channel>,
        outbound: ReceiverStream<NodeFrame>,
        outbox: &Arc<Outbox<NodeFrame>>,
    ) -> Result<(), TransportError> {
        let codec = ProstCodec::<NodeFrame, OrchestratorFrame>::default();
        let response = tokio::time::timeout(
            self.dial_timeout * 2,
            grpc.streaming(
                Request::new(outbound),
                PathAndQuery::from_static(NODE_CONNECT),
                codec,
            ),
        )
        .await
        .map_err(|_| {
            TransportError::Connect("the orchestrator did not accept the stream".into())
        })??;
        let mut inbound = response.into_inner();
        self.say(format!("connected to {}", self.cfg.address));
        let mut beat = tokio::time::interval(self.cfg.heartbeat_interval);
        let mut seq = 0u64;
        loop {
            tokio::select! {
                _ = beat.tick() => {
                    seq += 1;
                    let _ = outbox.push_coalesced(HEARTBEAT, guard(&self.session).heartbeat(seq));
                }
                message = inbound.message() => {
                    let Some(frame) = message? else { return Ok(()) };
                    let (jobs, close) = {
                        let mut session = guard(&self.session);
                        let out = session.on_frame(frame, now());
                        for f in out.frames {
                            let _ = outbox.push_reliable(f);
                        }
                        (out.jobs, out.close)
                    };
                    for job in jobs {
                        self.spawn_job(job, outbox.clone());
                    }
                    if close.is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Run a validated control job on a blocking thread, outside the session lock, and queue
    /// its response. Bounded: when all workers are busy the request is refused at once.
    fn spawn_job(&self, job: ControlJob, outbox: Arc<Outbox<NodeFrame>>) {
        let id = job.request_id();
        let Ok(permit) = self.jobs.clone().try_acquire_owned() else {
            let _ = outbox.push_reliable(error_frame(
                id,
                ErrorKindCode::RemoteCommandFailed,
                "node is busy",
            ));
            return;
        };
        let control = self.control.clone();
        tokio::spawn(async move {
            let work = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                job.execute(&*control, now())
            });
            let frame = match tokio::time::timeout(JOB_TIMEOUT, work).await {
                Ok(Ok(frame)) => frame,
                _ => error_frame(
                    id,
                    ErrorKindCode::RemoteCommandFailed,
                    "operation timed out",
                ),
            };
            let _ = outbox.push_reliable(frame);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_window_of_unreachable_failures_trips_the_watch_only_after_the_limit() {
        let t0 = Instant::now();
        let mut w = UnreachableWatch::new(Duration::from_secs(60));
        assert!(!w.record(true, t0));
        assert!(!w.record(true, t0 + Duration::from_secs(59)));
        assert!(w.record(true, t0 + Duration::from_secs(60)));
        assert_eq!(
            w.window(t0 + Duration::from_secs(61)),
            Duration::from_secs(61)
        );
    }

    #[test]
    fn any_other_outcome_restarts_the_window() {
        let t0 = Instant::now();
        let mut w = UnreachableWatch::new(Duration::from_secs(60));
        assert!(!w.record(true, t0));
        // The orchestrator being down, refusing or slow is not the condition: reset.
        assert!(!w.record(false, t0 + Duration::from_secs(50)));
        assert!(!w.record(true, t0 + Duration::from_secs(70)));
        assert!(!w.record(true, t0 + Duration::from_secs(120)));
        assert!(w.record(true, t0 + Duration::from_secs(130)));
    }

    #[test]
    fn jitter_adds_at_most_a_quarter() {
        for _ in 0..50 {
            let j = jittered(Duration::from_secs(60));
            assert!(
                j >= Duration::from_secs(60) && j <= Duration::from_secs(75),
                "{j:?}"
            );
        }
    }
}
