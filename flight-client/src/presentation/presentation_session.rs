// SPDX-License-Identifier: MIT

use super::command::Command;
use super::config::{failure_text, remote_text, PresentationConfig};
use super::frame::Frame;
use super::input_log::InputLog;
use super::keys::{Key, KeyFilter};
use super::outcome::PresentationOutcome;
use super::rebuild_budget::RebuildBudget;
use super::tile_link::{LinkState, Live, TileLink};
use crate::screens::{paint, EngineFailure, Geometry, ScreenModel};
use crate::session::{
    Attachment, Binding, FromRemote, Next, OpenFailure, OpenRequest, Reattach, SurfaceHost,
    ToRemote,
};
use crate::terminal::{LocalTerminal, TerminalEnd};
use flight_present::{cycle, neighbor, solve, Layout, Placement, Rect, Solved};
use flight_proto::{MAX_TERMINAL_DATA, MAX_TERMINAL_DIM};
use flight_state::SurfaceId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// The terminal shown, at the end, back to what the shell expects.
const RESET: &[u8] = b"\x1b[?1049l\x1b[?25h\x1b[0m\x1b[?2004l\x1b[?1l";
const HINT: &str = "Ctrl-Space: | or - split, t tab, x close, n/p tab, h j k l or o focus, < > + _ resize, a s show agent or shell, q leave";
/// A nudge of a surface's share, in percent of the whole.
const NUDGE: i32 = 10;
/// How long input that cannot move holds the keyboard before it is given up, counted.
const INPUT_STALL: Duration = Duration::from_secs(3);

enum Event {
    Opened {
        surface: SurfaceId,
        generation: u64,
        result: Result<Attachment, OpenFailure>,
    },
    Remote {
        surface: SurfaceId,
        generation: u64,
        what: FromRemote,
    },
    /// The far end finished with an attachment that was let go.
    Retired(SurfaceId),
}

/// Several surfaces on one terminal (ADR-011): each showing surface has its own attachment and
/// screen, in the tile a layout gives it. The layout decides only what is on screen; a surface
/// is attached while it is showing and not otherwise, and nothing about it changes when it is
/// moved, resized, hidden in a tab or shown again.
///
/// Guarantees:
/// - Input is an ordered log. Bytes go to the surface that had the keyboard when they were
///   typed, whether or not it is attached yet; they wait, bounded, are never delivered to a
///   different surface, and are counted if the surface goes away first.
/// - Each attachment hears its own tile's size, including after the terminal is resized.
/// - A surface is attached at most once at a time: a new attachment waits for the old one to
///   finish, so keys cannot overtake each other.
/// - A broken stream is attached again, a bounded number of times, to the same process only.
/// - One surface failing leaves the others running; the session ends when none can recover.
pub struct PresentationSession<H: SurfaceHost> {
    host: Arc<H>,
    cfg: PresentationConfig,
}

impl<H: SurfaceHost> PresentationSession<H> {
    pub fn new(host: Arc<H>, config: PresentationConfig) -> Self {
        Self { host, cfg: config }
    }

    pub async fn run(
        self,
        local: LocalTerminal,
        layout: Layout,
        size: (u16, u16),
    ) -> PresentationOutcome {
        let LocalTerminal {
            mut input,
            mut resizes,
            output,
        } = local;
        let (events_tx, mut events) = mpsc::channel::<Event>(64);
        let size = clamp(size);
        let mut run = Run {
            log: InputLog::new(self.cfg.input_limit),
            host: self.host,
            cfg: self.cfg,
            layout,
            size,
            keys: KeyFilter::default(),
            frame: Frame::new(size.0, size.1),
            tiles: HashMap::new(),
            retiring: Vec::new(),
            generation: 0,
            events_tx,
            dirty: true,
            frame_due: true,
            undelivered: 0,
            blocked_until: None,
            last_end: None,
        };
        let _ = output.send(b"\x1b[?1049h\x1b[2J".to_vec()).await;
        run.reconcile();
        let mut tick = tokio::time::interval(run.cfg.frame.max(Duration::from_millis(1)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut renewals = tokio::time::interval(run.cfg.lease_period);
        renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut resizes_open = true;
        let end = loop {
            if let Some(end) = run.advance() {
                break end;
            }
            run.watch_input_stall();
            let sender = run.sender_if_ready();
            let paint_due = run.dirty && run.frame_due;
            let step = tokio::select! {
                permit = async {
                    match sender {
                        Some((surface, tx)) => match tx.reserve_owned().await {
                            Ok(permit) => Ok((surface, permit)),
                            Err(_) => Err(surface),
                        },
                        None => std::future::pending().await,
                    }
                } => {
                    match permit {
                        Ok((surface, permit)) => run.send_one(&surface, permit),
                        Err(surface) => run.send_failed(&surface),
                    }
                    None
                }
                bytes = input.recv(), if run.log.has_room() => match bytes {
                    Some(bytes) => run.take_input(&bytes),
                    None => Some(TerminalEnd::UserLeft),
                },
                size = resizes.recv(), if resizes_open => {
                    match size {
                        Some(size) => { run.size = clamp(size); run.reconcile(); }
                        None => resizes_open = false,
                    }
                    None
                }
                Some(event) = events.recv() => { run.on_event(event); None }
                permit = async {
                    if paint_due { output.reserve().await } else { std::future::pending().await }
                } => {
                    if let Ok(permit) = permit {
                        permit.send(run.paint());
                    }
                    None
                }
                _ = tick.tick() => { run.frame_due = true; run.retry_due(); None }
                _ = renewals.tick() => run.renew(),
                () = async {
                    match run.blocked_until {
                        Some(until) => tokio::time::sleep_until(until).await,
                        None => std::future::pending().await,
                    }
                } => { run.give_up_blocked_input(); None }
            };
            if let Some(end) = step {
                break end;
            }
        };
        let outcome = run.finish(end);
        let _ = tokio::time::timeout(Duration::from_secs(1), output.send(RESET.to_vec())).await;
        outcome
    }
}

fn clamp((cols, rows): (u16, u16)) -> (u16, u16) {
    let max = u16::try_from(MAX_TERMINAL_DIM).unwrap_or(u16::MAX);
    (cols.clamp(1, max), rows.clamp(1, max))
}

struct Run<H: SurfaceHost> {
    host: Arc<H>,
    cfg: PresentationConfig,
    layout: Layout,
    size: (u16, u16),
    keys: KeyFilter,
    log: InputLog,
    frame: Frame,
    tiles: HashMap<SurfaceId, TileLink>,
    /// Surfaces whose earlier attachment is still finishing.
    retiring: Vec<SurfaceId>,
    generation: u64,
    events_tx: mpsc::Sender<Event>,
    dirty: bool,
    frame_due: bool,
    undelivered: usize,
    blocked_until: Option<Instant>,
    last_end: Option<TerminalEnd>,
}

impl<H: SurfaceHost> Run<H> {
    fn say(&self, text: &str) {
        (self.cfg.say)(text);
    }

    fn solved(&self) -> Solved {
        let area = Rect::new(0, 0, self.size.0, self.size.1);
        solve(&self.layout, area, &self.cfg.style)
    }

    /// Make the attachments match what is showing: attach what has a tile and none yet, let go
    /// of what has no tile, and tell each screen its tile's size. A tile too small to hold a
    /// screen (a terminal squeezed to nothing for a moment) changes nothing: its screen and its
    /// surface keep the last size they had, and the tile shows a placeholder until the viewport
    /// is big enough again. A terminal too small for any screen changes nothing at all.
    fn reconcile(&mut self) {
        if !self.tiles.is_empty() && Geometry::new(self.size.0, self.size.1).is_err() {
            // Nothing can be shown in this, and the next size may bring everything back: let go
            // of nothing, resize nothing. (Attaching, before anything is, still goes ahead, at the
            // standard size.)
            self.dirty = true;
            return;
        }
        let solved = self.solved();
        let showing: Vec<(SurfaceId, (u16, u16))> = solved
            .tiles
            .iter()
            .map(|t| (t.surface.clone(), (t.area.cols, t.area.rows)))
            .collect();
        let gone: Vec<SurfaceId> = self
            .tiles
            .keys()
            .filter(|s| !showing.iter().any(|(k, _)| k == *s))
            .cloned()
            .collect();
        for surface in gone {
            self.let_go(&surface);
        }
        for (surface, (cols, rows)) in showing {
            let fitted = Geometry::new(cols, rows).ok();
            match (self.tiles.get_mut(&surface), fitted) {
                (Some(tile), Some(geometry)) => tile.model.resize(geometry),
                (Some(_), None) => {}
                (None, geometry) => {
                    self.attach(surface, geometry.unwrap_or(Geometry::STANDARD), None, 0);
                }
            }
        }
        self.dirty = true;
    }

    fn let_go(&mut self, surface: &SurfaceId) {
        let Some(tile) = self.tiles.remove(surface) else {
            return;
        };
        self.undelivered = self.undelivered.saturating_add(self.log.discard(surface));
        let Some(retired) = tile.close() else { return };
        self.retiring.push(surface.clone());
        let (tx, wait, named) = (
            self.events_tx.clone(),
            self.cfg.retire_wait,
            surface.clone(),
        );
        tokio::spawn(async move {
            let _ = tokio::time::timeout(wait, retired).await;
            let _ = tx.send(Event::Retired(named)).await;
        });
    }

    /// Ask for an attachment of `surface` for a tile of `size`. `expect` is set when attaching
    /// again after a break: only that process will do.
    fn attach(
        &mut self,
        surface: SurfaceId,
        size: Geometry,
        expect: Option<Binding>,
        attempts: usize,
    ) {
        if self.retiring.contains(&surface) {
            return;
        }
        let Some(choice) = (self.cfg.resolve)(&surface) else {
            let mut model = ScreenModel::new(size);
            notice(&mut model, "this surface cannot be attached");
            self.last_end = Some(TerminalEnd::Lost("nothing to attach".to_owned()));
            self.put(surface, model, LinkState::Down);
            return;
        };
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        let (host, tx, timeout) = (
            self.host.clone(),
            self.events_tx.clone(),
            self.cfg.open_timeout,
        );
        let expect_kept = expect.clone();
        let request = OpenRequest {
            choice,
            cols: size.cols(),
            rows: size.rows(),
            expect,
        };
        let named = surface.clone();
        let task = tokio::spawn(async move {
            let result = match tokio::time::timeout(timeout, host.open(request)).await {
                Ok(result) => result,
                Err(_) => Err(OpenFailure::Unavailable(
                    "it did not answer in time".to_owned(),
                )),
            };
            let _ = tx
                .send(Event::Opened {
                    surface: named,
                    generation,
                    result,
                })
                .await;
        });
        let (model, budget) = match self.tiles.remove(&surface) {
            Some(old) => (old.model, old.budget),
            None => (ScreenModel::new(size), RebuildBudget::default()),
        };
        self.tiles.insert(
            surface,
            TileLink::opening(model, budget, generation, task, attempts, expect_kept),
        );
    }

    fn put(&mut self, surface: SurfaceId, model: ScreenModel, state: LinkState) {
        let generation = self.generation;
        self.tiles.insert(
            surface,
            TileLink {
                model,
                state,
                generation,
                budget: RebuildBudget::default(),
            },
        );
        self.dirty = true;
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Opened {
                surface,
                generation,
                result,
            } => self.on_opened(surface, generation, result),
            Event::Remote {
                surface,
                generation,
                what,
            } => self.on_remote(surface, generation, what),
            Event::Retired(surface) => {
                self.retiring.retain(|s| s != &surface);
                self.reconcile();
            }
        }
    }

    fn on_opened(
        &mut self,
        surface: SurfaceId,
        generation: u64,
        result: Result<Attachment, OpenFailure>,
    ) {
        let Some(tile) = self.tiles.get_mut(&surface) else {
            return;
        };
        if tile.generation != generation {
            return;
        }
        let (attempts, expect) = match &tile.state {
            LinkState::Opening {
                attempts, expect, ..
            } => (*attempts, expect.clone()),
            _ => return,
        };
        match result {
            Ok(attachment) => {
                let size = tile.model.size();
                let pump = pump(
                    surface.clone(),
                    generation,
                    attachment.from_remote,
                    self.events_tx.clone(),
                );
                tile.state = LinkState::Live(Live {
                    id: attachment.id,
                    binding: attachment.binding,
                    to_remote: attachment.to_remote,
                    pump,
                    sent_size: size,
                    attempts,
                    _guard: attachment.guard,
                    retired: attachment.retired,
                });
            }
            Err(failure) if Reattach::open_failure_is_transient(&failure) => {
                let why = failure_text(&failure);
                match Reattach::new(&self.cfg.reattach_delays).next(attempts) {
                    Next::After(delay) => {
                        let at = Instant::now()
                            .checked_add(delay)
                            .unwrap_or_else(Instant::now);
                        tile.state = LinkState::Retrying {
                            binding: expect,
                            attempts: attempts.saturating_add(1),
                            at,
                        };
                    }
                    Next::GiveUp => {
                        notice(&mut tile.model, &format!("connection lost: {why}"));
                        tile.state = LinkState::Down;
                        self.last_end = Some(TerminalEnd::Lost(why));
                    }
                }
            }
            Err(failure) => {
                notice(&mut tile.model, &failure_text(&failure));
                tile.state = LinkState::Down;
                self.last_end = Some(TerminalEnd::Lost(failure_text(&failure)));
            }
        }
        self.dirty = true;
    }

    /// The emulator failed on what `surface` wrote, and its screen was emptied. Only this tile is
    /// affected: its attachment is dropped and made again to the same process, whose first
    /// output is a full drawing, so the screen is rebuilt from the live surface. The surface
    /// itself (its tmux session and process) is never touched. If that keeps failing, the tile
    /// is given up and says so; the other tiles and the session carry on.
    fn screen_failed(&mut self, surface: &SurfaceId, failure: &EngineFailure) {
        self.say(&format!("{surface}: {failure}"));
        let Some(tile) = self.tiles.get_mut(surface) else {
            return;
        };
        let again = tile.budget.rebuild();
        let LinkState::Live(live) = &tile.state else {
            return;
        };
        live.pump.abort();
        if again {
            tile.state = LinkState::Retrying {
                binding: Some(live.binding.clone()),
                attempts: live.attempts,
                at: Instant::now(),
            };
        } else {
            let why = "its screen cannot follow what it writes".to_owned();
            notice(&mut tile.model, &why);
            tile.state = LinkState::Down;
            self.last_end = Some(TerminalEnd::Lost(why));
        }
        self.dirty = true;
    }

    /// The stream of `surface` broke while the process may still be there: attach the same
    /// process again, a bounded number of times, then give the tile up.
    fn stream_broke(&mut self, surface: &SurfaceId, why: String) {
        let Some(tile) = self.tiles.get_mut(surface) else {
            return;
        };
        let LinkState::Live(live) = &tile.state else {
            return;
        };
        live.pump.abort();
        let (binding, attempts) = (live.binding.clone(), live.attempts);
        match Reattach::new(&self.cfg.reattach_delays).next(attempts) {
            Next::After(delay) => {
                let at = Instant::now()
                    .checked_add(delay)
                    .unwrap_or_else(Instant::now);
                tile.state = LinkState::Retrying {
                    binding: Some(binding),
                    attempts: attempts.saturating_add(1),
                    at,
                };
            }
            Next::GiveUp => {
                notice(&mut tile.model, &format!("connection lost: {why}"));
                tile.state = LinkState::Down;
                self.last_end = Some(TerminalEnd::Lost(why));
            }
        }
        self.dirty = true;
    }

    fn on_remote(&mut self, surface: SurfaceId, generation: u64, what: FromRemote) {
        let Some(tile) = self.tiles.get_mut(&surface) else {
            return;
        };
        if tile.generation != generation {
            return;
        }
        match what {
            FromRemote::Data(bytes) => {
                if let LinkState::Live(live) = &mut tile.state {
                    live.attempts = 0;
                }
                match tile.model.feed(&bytes) {
                    Ok(()) => tile.budget.followed(bytes.len()),
                    Err(failure) => self.screen_failed(&surface, &failure),
                }
            }
            FromRemote::Exit { reason, .. } if Reattach::exit_is_a_break(reason) => {
                self.stream_broke(&surface, "the node's connection was lost".to_owned());
            }
            FromRemote::Exit { reason, status } => {
                let end = FromRemote::Exit { reason, status };
                notice(&mut tile.model, &remote_text(&end));
                if let LinkState::Live(live) = &tile.state {
                    live.pump.abort();
                }
                tile.state = LinkState::Down;
                self.last_end = Some(TerminalEnd::Exited { reason, status });
            }
            FromRemote::Lost(why) => self.stream_broke(&surface, why),
        }
        self.dirty = true;
    }

    /// Attach again the surfaces whose wait is over.
    fn retry_due(&mut self) {
        let now = Instant::now();
        let due: Vec<(SurfaceId, Option<Binding>, usize)> = self
            .tiles
            .iter()
            .filter_map(|(s, t)| match &t.state {
                LinkState::Retrying {
                    binding,
                    attempts,
                    at,
                } if *at <= now => Some((s.clone(), binding.clone(), *attempts)),
                _ => None,
            })
            .collect();
        for (surface, binding, attempts) in due {
            let size = self
                .tiles
                .get(&surface)
                .map_or(Geometry::STANDARD, |t| t.model.geometry());
            self.attach(surface, size, binding, attempts);
        }
    }

    /// Apply what needs no attachment (nothing, now) and decide whether the session is over:
    /// every showing surface is down for good.
    fn advance(&mut self) -> Option<TerminalEnd> {
        // Input for a surface that is down for good will never be delivered.
        while let Some(front) = self.log.front().cloned() {
            let gone = self
                .tiles
                .get(&front)
                .is_none_or(|t| matches!(t.state, LinkState::Down));
            if !gone {
                break;
            }
            self.undelivered = self.undelivered.saturating_add(self.log.discard(&front));
        }
        let none_left = !self.tiles.is_empty() && self.tiles.values().all(|t| !t.may_recover());
        none_left.then(|| {
            self.last_end
                .clone()
                .unwrap_or_else(|| TerminalEnd::Lost("every surface is down".to_owned()))
        })
    }

    fn take_input(&mut self, bytes: &[u8]) -> Option<TerminalEnd> {
        for key in self.keys.keys(bytes) {
            match key {
                Key::Data(data) => {
                    let focus = self.layout.focus().clone();
                    self.log.push(&focus, data);
                }
                Key::Command(command) => self.apply(command),
                Key::Hint => self.say(HINT),
                // Leaving works even when nothing is taking input.
                Key::Leave => return Some(TerminalEnd::UserLeft),
            }
        }
        None
    }

    /// What needs sending to an attachment, and the channel to wait on for room: a tile's new
    /// size first, then the oldest typed bytes if their surface is attached.
    fn sender_if_ready(&self) -> Option<(SurfaceId, mpsc::Sender<ToRemote>)> {
        let resize = self.tiles.iter().find_map(|(s, t)| {
            let live = t.live()?;
            (live.sent_size != t.model.size()).then(|| (s.clone(), live.to_remote.clone()))
        });
        resize.or_else(|| {
            let front = self.log.front()?;
            let live = self.tiles.get(front)?.live()?;
            Some((front.clone(), live.to_remote.clone()))
        })
    }

    fn send_one(&mut self, surface: &SurfaceId, permit: mpsc::OwnedPermit<ToRemote>) {
        let Some(tile) = self.tiles.get_mut(surface) else {
            return;
        };
        let size = tile.model.size();
        if let LinkState::Live(live) = &mut tile.state {
            if live.sent_size != size {
                live.sent_size = size;
                permit.send(ToRemote::Resize(size.0, size.1));
                return;
            }
        }
        if self.log.front() == Some(surface) {
            if let Some((_, data)) = self.log.pop(MAX_TERMINAL_DATA) {
                permit.send(ToRemote::Data(data));
            }
        }
    }

    /// The channel to an attachment is closed: its stream is gone.
    fn send_failed(&mut self, surface: &SurfaceId) {
        let generation = self.tiles.get(surface).map_or(0, |t| t.generation);
        self.on_remote(
            surface.clone(),
            generation,
            FromRemote::Lost("the stream is closed".to_owned()),
        );
    }

    fn watch_input_stall(&mut self) {
        if self.log.has_room() {
            self.blocked_until = None;
        } else if self.blocked_until.is_none() {
            self.blocked_until = Instant::now().checked_add(INPUT_STALL);
        }
    }

    /// Input has not moved for too long: give up what is stuck at the front, counted, and say so.
    fn give_up_blocked_input(&mut self) {
        self.blocked_until = None;
        if let Some(front) = self.log.front().cloned() {
            let dropped = self.log.discard(&front);
            self.undelivered = self.undelivered.saturating_add(dropped);
            self.say(&format!(
                "{front} is not taking input; {dropped} typed byte(s) were dropped"
            ));
        }
    }

    fn renew(&mut self) -> Option<TerminalEnd> {
        for tile in self.tiles.values() {
            if let Some(live) = tile.live() {
                if let Err(why) = self.host.renew(&live.id) {
                    return Some(TerminalEnd::Lost(format!(
                        "the control connection is gone: {why}"
                    )));
                }
            }
        }
        None
    }

    fn apply(&mut self, command: Command) {
        let focus = self.layout.focus().clone();
        let next_free = self
            .cfg
            .surfaces
            .iter()
            .find(|s| !self.layout.contains(s))
            .cloned();
        let result = match command {
            Command::Split(axis) => match next_free {
                Some(new) => self.layout.split(&focus, axis, new, Placement::After),
                None => return self.say("every surface of this workspace is already shown"),
            },
            Command::NewTab => match next_free {
                Some(new) => self.layout.add_tab(&focus, new),
                None => return self.say("every surface of this workspace is already shown"),
            },
            Command::Close => self.layout.remove(&focus),
            Command::StepTab(forward) => Ok(self.layout.step_tab(forward)),
            Command::Focus(direction) => {
                let solved = self.solved();
                match neighbor(&solved, &focus, direction) {
                    Some(to) => self.layout.focus_on(&to),
                    None => return,
                }
            }
            Command::FocusNext => match cycle(&self.solved(), &focus, true) {
                Some(to) => self.layout.focus_on(&to),
                None => return,
            },
            Command::Grow(axis, bigger) => {
                self.layout
                    .resize(&focus, axis, if bigger { NUDGE } else { -NUDGE })
            }
            Command::ShowHere(which) => match self.cfg.surface_for(which) {
                Some(s) if s == focus => return,
                Some(s) if self.layout.contains(&s) => self.layout.focus_on(&s),
                Some(s) => self.layout.replace(&focus, s),
                None => return,
            },
        };
        match result {
            Ok(layout) => {
                self.layout = layout;
                self.reconcile();
            }
            Err(why) => self.say(&format!("not possible: {why}")),
        }
    }

    /// The bytes that bring the real terminal to the current picture.
    fn paint(&mut self) -> Vec<u8> {
        self.dirty = false;
        self.frame_due = false;
        let solved = self.solved();
        let mut buf = self.frame.blank(self.size.0, self.size.1);
        let tiles = &self.tiles;
        let label = &self.cfg.label;
        let painted = paint(
            &mut buf,
            &solved,
            &|s| tiles.get(s).map(|t| &t.model),
            &|s| label(s),
            &self.cfg.theme,
        );
        let modes = tiles.get(self.layout.focus()).map(|t| t.model.modes());
        self.frame.bytes(buf, painted, modes)
    }

    fn finish(mut self, end: TerminalEnd) -> PresentationOutcome {
        for (_, tile) in self.tiles.drain() {
            let _ = tile.close();
        }
        PresentationOutcome {
            end,
            layout: self.layout,
            undelivered: self.undelivered.saturating_add(self.log.queued_bytes()),
        }
    }
}

/// Copy one attachment's output into the session, tagged so a report from an attachment that
/// was already replaced is recognised and ignored.
fn pump(
    surface: SurfaceId,
    generation: u64,
    mut from_remote: mpsc::Receiver<FromRemote>,
    events: mpsc::Sender<Event>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let what = match from_remote.recv().await {
                Some(what) => what,
                None => FromRemote::Lost("the stream ended".to_owned()),
            };
            let last = !matches!(what, FromRemote::Data(_));
            let event = Event::Remote {
                surface: surface.clone(),
                generation,
                what,
            };
            if events.send(event).await.is_err() || last {
                return;
            }
        }
    })
}

/// Replace what a screen shows with a line of words, so a surface that is not coming back says
/// why where it was.
fn notice(model: &mut ScreenModel, text: &str) {
    let _ = model.feed(format!("\x1b[0m\x1b[2J\x1b[H{text}").as_bytes());
}
