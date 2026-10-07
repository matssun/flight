// SPDX-License-Identifier: MIT

use super::keys::action_for;
use super::worker::{Cmd, Msg, Worker};
use crate::collect::Backend;
use crate::render::render;
use crate::view::{Effect, ViewModel};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::time::Duration;

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
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// The terminal loop: owns the terminal and the worker; everything else is pure.
pub fn run(backend: impl Backend + 'static, refresh_every: Duration) -> io::Result<Exit> {
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut worker = Worker::spawn(Box::new(backend), refresh_every);
    let exit = event_loop(&mut terminal, &mut worker);
    worker.shutdown();
    exit
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    worker: &mut Worker,
) -> io::Result<Exit> {
    let mut vm = ViewModel::new();
    loop {
        terminal.draw(|f| render(f, &vm))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(action) = action_for(key) {
                        let effect = vm_apply(&mut vm, action);
                        if let Some(exit) = handle(worker, effect) {
                            return Ok(exit);
                        }
                    }
                }
            }
        }
        while let Ok(msg) = worker.rx.try_recv() {
            if let Some(exit) = on_message(&mut vm, worker, msg) {
                return Ok(exit);
            }
        }
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
        Msg::Switched(Ok(())) => Some(Exit::Switched),
        Msg::Switched(Err(e)) => {
            vm.set_message(Some(format!("cannot switch: {e}")));
            None
        }
    }
}
