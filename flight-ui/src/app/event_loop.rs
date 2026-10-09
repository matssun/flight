// SPDX-License-Identifier: MIT

use super::keys::action_for;
use super::worker::{Cmd, Msg, Worker};
use crate::collect::Backend;
use crate::render::{render, session_at};
use crate::snapshot::WorkspaceKey;
use crate::view::{Action, Effect, SurfaceChoice, ViewModel};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

/// Two clicks on the same session this close together open it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// How the dashboard ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Quit,
    /// The user jumped to a pane (the dashboard exits, like Fleet's popup).
    Switched,
}

/// Restores the terminal on every exit path, including a panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

/// The terminal loop: owns the terminal and the worker; everything else is pure.
pub fn run(backend: impl Backend + 'static, refresh_every: Duration) -> io::Result<Exit> {
    run_with_notice(backend, refresh_every, None)
}

/// [`run`], with a line to show in the dashboard at first (for example why a terminal ended).
pub fn run_with_notice(
    backend: impl Backend + 'static,
    refresh_every: Duration,
    notice: Option<String>,
) -> io::Result<Exit> {
    run_with_start(
        backend,
        refresh_every,
        Start {
            notice,
            ..Start::default()
        },
    )
}

/// How the dashboard begins: with a line to show (for example why a terminal ended), and
/// optionally a surface to open the moment it is listed (the user left a terminal in order to
/// switch to its workspace's other surface).
#[derive(Debug, Clone, Default)]
pub struct Start {
    pub notice: Option<String>,
    pub resume: Option<(WorkspaceKey, SurfaceChoice)>,
    /// The workspace to put the cursor on.
    pub select: Option<WorkspaceKey>,
}

/// [`run`], beginning as `start` says.
pub fn run_with_start(
    backend: impl Backend + 'static,
    refresh_every: Duration,
    start: Start,
) -> io::Result<Exit> {
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut worker = Worker::spawn(Box::new(backend), refresh_every);
    let exit = event_loop(&mut terminal, &mut worker, start);
    worker.shutdown();
    exit
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    worker: &mut Worker,
    start: Start,
) -> io::Result<Exit> {
    let mut vm = ViewModel::new();
    vm.set_message(start.notice);
    if let Some(key) = start.select {
        vm.point_at(key);
    }
    if let Some((key, choice)) = start.resume {
        vm.resume(key, choice);
    }
    let mut last_click: Option<(flight_state::PaneRef, Instant)> = None;
    loop {
        terminal.draw(|f| render(f, &vm))?;
        if event::poll(Duration::from_millis(100))? {
            let action = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    action_for(key, vm.input_mode())
                }
                Event::Mouse(m) => {
                    let size = terminal.size()?;
                    mouse_action(
                        &vm,
                        Rect::new(0, 0, size.width, size.height),
                        m,
                        &mut last_click,
                    )
                }
                _ => None,
            };
            if let Some(action) = action {
                let effect = vm_apply(&mut vm, action);
                if let Some(exit) = handle(worker, effect) {
                    return Ok(exit);
                }
            }
        } else {
            vm.tick();
            if vm.is_opening() {
                let _ = worker.tx.send(Cmd::Refresh);
            }
        }
        while let Ok(msg) = worker.rx.try_recv() {
            if let Some(exit) = on_message(&mut vm, worker, msg) {
                return Ok(exit);
            }
        }
    }
}

/// A click selects the session under it; a second click on the same one opens it; the wheel
/// moves the selection.
fn mouse_action(
    vm: &ViewModel,
    area: Rect,
    m: MouseEvent,
    last_click: &mut Option<(flight_state::PaneRef, Instant)>,
) -> Option<Action> {
    match m.kind {
        MouseEventKind::ScrollUp => Some(Action::Up),
        MouseEventKind::ScrollDown => Some(Action::Down),
        MouseEventKind::Down(MouseButton::Left) => {
            let pane = session_at(vm, area, m.column, m.row)?;
            let again =
                matches!(last_click, Some((p, at)) if *p == pane && at.elapsed() < DOUBLE_CLICK);
            if again {
                *last_click = None;
                Some(Action::Switch)
            } else {
                *last_click = Some((pane.clone(), Instant::now()));
                Some(Action::Select(pane))
            }
        }
        _ => None,
    }
}

fn vm_apply(vm: &mut ViewModel, action: crate::view::Action) -> Effect {
    vm.set_message(None);
    vm.apply(action)
}

fn handle(worker: &Worker, effect: Effect) -> Option<Exit> {
    match effect {
        Effect::None => {}
        Effect::Quit => return Some(Exit::Quit),
        Effect::Refresh => {
            let _ = worker.tx.send(Cmd::Refresh);
        }
        Effect::Select(p) => {
            let _ = worker.tx.send(Cmd::Select(p));
        }
        Effect::Switch(p) => {
            let _ = worker.tx.send(Cmd::Switch(p));
        }
        Effect::Create(request) => {
            let _ = worker.tx.send(Cmd::Create(request));
        }
        Effect::CreateSurface(request) => {
            let _ = worker.tx.send(Cmd::CreateSurface(request));
        }
    }
    None
}

fn on_message(vm: &mut ViewModel, worker: &Worker, msg: Msg) -> Option<Exit> {
    match msg {
        Msg::Snapshot(s) => {
            let effect = vm.apply_snapshot(s);
            handle(worker, effect)
        }
        Msg::Preview(p) => {
            vm.apply_preview(p);
            None
        }
        Msg::Created(request, result) => {
            vm.apply_created(&request, result);
            None
        }
        Msg::SurfaceCreated(request, result) => {
            let effect = vm.apply_surface_created(&request, result);
            handle(worker, effect)
        }
        Msg::Switched(Ok(())) => Some(Exit::Switched),
        Msg::Switched(Err(e)) => {
            vm.set_message(Some(format!("cannot switch: {e}")));
            None
        }
    }
}
