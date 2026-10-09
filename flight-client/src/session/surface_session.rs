// SPDX-License-Identifier: MIT

use crate::session::pump::{self, Ended, PumpEnd};
use crate::session::{
    Attachment, Binding, FromRemote, InputEvent, InputQueue, OpenFailure, OpenRequest,
    SessionOutcome, SurfaceHost, ToRemote,
};
use crate::terminal::{EscapeFilter, LocalTerminal, TerminalEnd};
use flight_proto::{ExitReasonCode, MAX_TERMINAL_DATA, MAX_TERMINAL_DIM};
use flight_ui::SurfaceChoice;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Said when `Ctrl-Space` is followed by a key that means nothing.
const HINT: &str =
    "[Ctrl-Space: q dashboard, a agent, s shell; Ctrl-Space Ctrl-Space sends a literal Ctrl-Space]";

/// Limits and timings of a session. The defaults are the ones the product uses.
pub struct SessionConfig {
    /// Bytes of typed input held while an attachment is not ready. Reading the keyboard stops
    /// at this bound; nothing is dropped.
    pub input_limit: usize,
    /// How long a surface is given to attach.
    pub open_timeout: Duration,
    /// How long input may be unable to move before the session gives up on the surface.
    pub input_stall: Duration,
    /// How often the presentation tells the orchestrator it is alive.
    pub lease_period: Duration,
    /// Waits before each attempt to re-attach a lost surface; its length is the attempt limit.
    pub reattach_delays: Vec<Duration>,
    /// Written to the terminal between two surfaces: leave the alternate screen, show the
    /// cursor, reset attributes and mouse modes the previous surface may have left on.
    pub reset: Vec<u8>,
    /// Where one-line notices for the user go (not into the surface's own output).
    pub say: Arc<dyn Fn(&str) + Send + Sync>,
}

impl SessionConfig {
    pub fn new(say: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        Self {
            input_limit: 64 * 1024,
            open_timeout: Duration::from_secs(15),
            input_stall: Duration::from_secs(3),
            lease_period: crate::terminal::LEASE_PERIOD,
            reattach_delays: vec![
                Duration::from_millis(250),
                Duration::from_secs(1),
                Duration::from_secs(3),
            ],
            reset: b"\x1b[?1049l\x1b[?25h\x1b[0m\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l"
                .to_vec(),
            say,
        }
    }
}

/// The first surface of a session: a terminal the dashboard already asked for.
pub struct SessionStart {
    pub id: Vec<u8>,
    pub choice: SurfaceChoice,
    /// The pane and process the terminal was asked for.
    pub binding: Binding,
    /// Input that arrived before the session could read the keyboard, in the order typed.
    pub typed_ahead: Vec<u8>,
    pub size: (u16, u16),
}

/// The user's terminal connected, one surface at a time, to the surfaces of a workspace.
///
/// Guarantees (ADR-009):
/// - Input is an ordered log. The bytes typed after `Ctrl-Space s` belong to the shell, from the
///   moment they are typed, whether or not the shell is attached yet. They wait, bounded, and are
///   never delivered to the surface that was shown before, nor to a different surface if the
///   shell cannot be reached; they are counted as undelivered instead.
/// - A switch attaches the new surface before the old one is let go. If it fails the user stays
///   where they were, with a notice.
/// - A lost stream is re-attached a bounded number of times, to the same process only.
/// - The keyboard is read only while the queue has room, so a stalled surface pushes back to the
///   terminal's own input buffer rather than growing memory or dropping keys.
pub struct SurfaceSession<H: SurfaceHost> {
    host: Arc<H>,
    config: SessionConfig,
}

struct Live {
    choice: SurfaceChoice,
    id: Vec<u8>,
    binding: Binding,
    to_remote: mpsc::Sender<ToRemote>,
    pump: JoinHandle<()>,
    generation: u64,
    /// The size the attachment last heard of.
    sent_size: (u16, u16),
    _guard: Option<Box<dyn Send>>,
}

struct Opening {
    choice: SurfaceChoice,
    expect: Option<Binding>,
    task: JoinHandle<Result<Attachment, OpenFailure>>,
    size: (u16, u16),
}

struct Retry {
    choice: SurfaceChoice,
    expect: Option<Binding>,
    at: Instant,
}

impl<H: SurfaceHost> SurfaceSession<H> {
    pub fn new(host: Arc<H>, config: SessionConfig) -> Self {
        Self { host, config }
    }

    /// Run until the user leaves, a surface ends, or the link is gone.
    pub async fn run(self, local: LocalTerminal, start: SessionStart) -> SessionOutcome {
        let LocalTerminal {
            mut input,
            mut resizes,
            output,
        } = local;
        let (ended_tx, mut ended_rx) = mpsc::channel::<Ended>(4);
        let mut run = Run {
            queue: InputQueue::new(self.config.input_limit),
            filter: EscapeFilter::showing(Some(start.choice)),
            size: start.size,
            host: self.host,
            cfg: self.config,
            output,
            ended_tx,
            live: None,
            opening: None,
            retry: None,
            shown: None,
            generation: 0,
            attempts: 0,
            undelivered: 0,
            blocked_until: None,
        };
        run.connect_first(start.id, start.choice, start.binding);
        for event in run.filter.events(&start.typed_ahead) {
            run.queue.push(event);
        }
        let mut renewals = tokio::time::interval(run.cfg.lease_period);
        renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut resizes_open = true;
        let end = loop {
            if let Some(end) = run.advance() {
                break end;
            }
            run.watch_input_stall();
            let sender = run.sender_if_ready();
            let step = tokio::select! {
                permit = async {
                    match sender {
                        Some(tx) => tx.reserve_owned().await,
                        None => std::future::pending().await,
                    }
                } => match permit {
                    Ok(permit) => { run.send_one(permit); None }
                    Err(_) => run.lost("the stream is closed".to_owned()),
                },
                bytes = input.recv(), if run.queue.has_room() => match bytes {
                    Some(bytes) => run.take_input(&bytes),
                    None => Some(TerminalEnd::UserLeft),
                },
                size = resizes.recv(), if resizes_open => {
                    match size {
                        Some((cols, rows)) => run.size = clamp_size(cols, rows),
                        None => resizes_open = false,
                    }
                    None
                }
                Some(ended) = ended_rx.recv() => run.on_ended(ended),
                done = async {
                    match run.opening.as_mut() {
                        Some(opening) => (&mut opening.task).await,
                        None => std::future::pending().await,
                    }
                } => run.on_opened(done),
                () = async {
                    match &run.retry {
                        Some(retry) => tokio::time::sleep_until(retry.at).await,
                        None => std::future::pending().await,
                    }
                } => { run.retry_open(); None }
                () = async {
                    match run.blocked_until {
                        Some(until) => tokio::time::sleep_until(until).await,
                        None => std::future::pending().await,
                    }
                } => Some(TerminalEnd::Lost("the surface is not taking input".to_owned())),
                _ = renewals.tick() => run.renew(),
            };
            if let Some(end) = step {
                break end;
            }
        };
        run.finish(end)
    }
}

/// `delay` from now, saturating instead of overflowing.
fn after(delay: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(delay).unwrap_or(now)
}

fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    let max = u16::try_from(MAX_TERMINAL_DIM).unwrap_or(u16::MAX);
    (cols.clamp(1, max), rows.clamp(1, max))
}

struct Run<H: SurfaceHost> {
    host: Arc<H>,
    cfg: SessionConfig,
    queue: InputQueue,
    filter: EscapeFilter,
    size: (u16, u16),
    output: mpsc::Sender<Vec<u8>>,
    ended_tx: mpsc::Sender<Ended>,
    live: Option<Live>,
    opening: Option<Opening>,
    retry: Option<Retry>,
    shown: Option<SurfaceChoice>,
    generation: u64,
    /// Attempts made to re-attach after the current outage.
    attempts: usize,
    undelivered: usize,
    /// When input that cannot move gives up the surface.
    blocked_until: Option<Instant>,
}

impl<H: SurfaceHost> Run<H> {
    fn say(&self, text: &str) {
        (self.cfg.say)(text);
    }

    fn spawn_open(
        &mut self,
        choice: SurfaceChoice,
        expect: Option<Binding>,
        connect: Option<(Vec<u8>, Binding)>,
    ) {
        let (host, timeout, size) = (self.host.clone(), self.cfg.open_timeout, self.size);
        let request = OpenRequest {
            choice,
            cols: size.0,
            rows: size.1,
            expect: expect.clone(),
        };
        let task = tokio::spawn(async move {
            let attach = async {
                match connect {
                    Some((id, binding)) => host.connect(id, choice, binding).await,
                    None => host.open(request).await,
                }
            };
            match tokio::time::timeout(timeout, attach).await {
                Ok(result) => result,
                Err(_) => Err(OpenFailure::Unavailable(
                    "it did not answer in time".to_owned(),
                )),
            }
        });
        self.opening = Some(Opening {
            choice,
            expect,
            task,
            size,
        });
    }

    fn connect_first(&mut self, id: Vec<u8>, choice: SurfaceChoice, binding: Binding) {
        self.spawn_open(choice, None, Some((id, binding)));
    }

    /// Apply the events at the front of the input that need no attachment. A switch is applied
    /// the moment it reaches the front, so every byte typed before it has already been delivered.
    fn advance(&mut self) -> Option<TerminalEnd> {
        loop {
            match self.queue.front() {
                Some(InputEvent::Leave) => return Some(TerminalEnd::UserLeft),
                Some(InputEvent::Hint) => {
                    self.queue.pop(usize::MAX);
                    self.say(HINT);
                }
                Some(InputEvent::Switch(_)) => {
                    if let Some(InputEvent::Switch(choice)) = self.queue.pop(usize::MAX) {
                        self.begin_switch(choice);
                    }
                }
                _ => return None,
            }
        }
    }

    fn begin_switch(&mut self, choice: SurfaceChoice) {
        if let Some(opening) = self.opening.take() {
            opening.task.abort();
        }
        self.retry = None;
        if self.live.as_ref().is_some_and(|l| l.choice == choice) {
            return;
        }
        self.attempts = 0;
        self.spawn_open(choice, None, None);
    }

    /// Whether anything can be sent to the attachment now, and if so a handle to wait for room.
    fn sender_if_ready(&self) -> Option<mpsc::Sender<ToRemote>> {
        let live = self.live.as_ref()?;
        let resize = live.sent_size != self.size;
        let data =
            self.opening.is_none() && matches!(self.queue.front(), Some(InputEvent::Data(_)));
        (resize || data).then(|| live.to_remote.clone())
    }

    fn send_one(&mut self, permit: mpsc::OwnedPermit<ToRemote>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.sent_size != self.size {
            live.sent_size = self.size;
            permit.send(ToRemote::Resize(self.size.0, self.size.1));
        } else if self.opening.is_none() {
            if let Some(InputEvent::Data(bytes)) = self.queue.pop(MAX_TERMINAL_DATA) {
                permit.send(ToRemote::Data(bytes));
            }
        }
    }

    fn take_input(&mut self, bytes: &[u8]) -> Option<TerminalEnd> {
        for event in self.filter.events(bytes) {
            // Leaving works even when the surface is not taking input.
            if event == InputEvent::Leave {
                return Some(TerminalEnd::UserLeft);
            }
            self.queue.push(event);
        }
        None
    }

    fn watch_input_stall(&mut self) {
        if self.queue.has_room() {
            self.blocked_until = None;
        } else if self.blocked_until.is_none() {
            self.blocked_until = Some(after(self.cfg.input_stall));
        }
    }

    fn on_opened(
        &mut self,
        done: Result<Result<Attachment, OpenFailure>, tokio::task::JoinError>,
    ) -> Option<TerminalEnd> {
        let opening = self.opening.take()?;
        let result = done.unwrap_or_else(|e| Err(OpenFailure::Unavailable(e.to_string())));
        match result {
            Ok(attachment) => {
                self.go_live(opening, attachment);
                None
            }
            Err(failure) => self.open_failed(opening, failure),
        }
    }

    fn go_live(&mut self, opening: Opening, attachment: Attachment) {
        // The new surface is up before the old one is let go.
        if let Some(old) = self.live.take() {
            close(old);
        }
        self.generation = self.generation.saturating_add(1);
        let reset = self.shown.is_some().then(|| self.cfg.reset.clone());
        let Attachment {
            id,
            binding,
            to_remote,
            from_remote,
            guard,
        } = attachment;
        let pump = pump::spawn(
            self.generation,
            from_remote,
            self.output.clone(),
            self.ended_tx.clone(),
            reset,
        );
        self.live = Some(Live {
            choice: opening.choice,
            id,
            binding,
            to_remote,
            pump,
            generation: self.generation,
            sent_size: opening.size,
            _guard: guard,
        });
        self.shown = Some(opening.choice);
        self.attempts = 0;
        self.filter.set_showing(Some(opening.choice));
    }

    fn open_failed(&mut self, opening: Opening, failure: OpenFailure) -> Option<TerminalEnd> {
        let (OpenFailure::Refused(why) | OpenFailure::Unavailable(why)) = &failure;
        if let Some(live) = &self.live {
            // A switch that failed: stay where the user is, and do not hand what they typed for
            // the other surface to this one.
            self.undelivered = self.undelivered.saturating_add(self.queue.discard_data());
            self.filter.set_showing(Some(live.choice));
            self.say(&format!(
                "cannot show the {}: {why}",
                opening.choice.label()
            ));
            return None;
        }
        match failure {
            OpenFailure::Unavailable(_) if self.attempts < self.cfg.reattach_delays.len() => {
                self.schedule_retry(opening.choice, opening.expect, why);
                None
            }
            _ => Some(TerminalEnd::Lost(why.clone())),
        }
    }

    fn schedule_retry(&mut self, choice: SurfaceChoice, expect: Option<Binding>, why: &str) {
        let delay = self
            .cfg
            .reattach_delays
            .get(self.attempts)
            .copied()
            .unwrap_or_default();
        self.attempts = self.attempts.saturating_add(1);
        self.say(&format!("connection lost ({why}); trying again"));
        self.retry = Some(Retry {
            choice,
            expect,
            at: after(delay),
        });
    }

    fn retry_open(&mut self) {
        if let Some(retry) = self.retry.take() {
            self.spawn_open(retry.choice, retry.expect, None);
        }
    }

    fn on_ended(&mut self, ended: Ended) -> Option<TerminalEnd> {
        if self.live.as_ref().map(|l| l.generation) != Some(ended.generation) {
            return None;
        }
        match ended.end {
            PumpEnd::LocalGone => Some(TerminalEnd::UserLeft),
            PumpEnd::Remote(FromRemote::Exit { reason, status }) => {
                if reason == ExitReasonCode::NodeLost {
                    self.lost("the node's connection was lost".to_owned())
                } else {
                    Some(TerminalEnd::Exited { reason, status })
                }
            }
            PumpEnd::Remote(FromRemote::Lost(why)) => self.lost(why),
            PumpEnd::Remote(FromRemote::Data(_)) => None,
        }
    }

    /// The stream broke: attach the same process again, a bounded number of times.
    fn lost(&mut self, why: String) -> Option<TerminalEnd> {
        let live = self.live.take()?;
        let (choice, binding) = (live.choice, live.binding.clone());
        close(live);
        if self.attempts >= self.cfg.reattach_delays.len() {
            return Some(TerminalEnd::Lost(why));
        }
        self.schedule_retry(choice, Some(binding), &why);
        None
    }

    fn renew(&mut self) -> Option<TerminalEnd> {
        let live = self.live.as_ref()?;
        self.host
            .renew(&live.id)
            .err()
            .map(|why| TerminalEnd::Lost(format!("the control connection is gone: {why}")))
    }

    fn finish(mut self, end: TerminalEnd) -> SessionOutcome {
        if let Some(opening) = self.opening.take() {
            opening.task.abort();
        }
        if let Some(live) = self.live.take() {
            close(live);
        }
        SessionOutcome {
            end,
            shown: self.shown,
            undelivered: self.undelivered.saturating_add(self.queue.queued_bytes()),
        }
    }
}

/// Let an attachment go: tell it, best effort, and stop reading its output.
fn close(live: Live) {
    let _ = live.to_remote.try_send(ToRemote::Close);
    live.pump.abort();
}
