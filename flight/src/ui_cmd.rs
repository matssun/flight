// SPDX-License-Identifier: MIT

use crate::args::{config_dir, now, Args};
use crate::join_cmd::join_command;
use crate::roles::ui_dir;
use flight_client::{
    remember, run_presentation, run_session, side_by_side, starting_layout, ClientConfig, Handoff,
    LayoutStore, OrchestratedBackend, SessionRequest, Switcher, TerminalEnd,
};
use flight_proto::RoleCode;
use flight_state::SurfaceId;
use flight_ui::{render_to_string, run_with_start, Backend, Exit, Start, ViewModel};
use std::time::Duration;

pub const USAGE: &str = "usage: flight ui <command>

  join <bundle> [--name NAME] [--bundle-file PATH] [--config-dir DIR]
        enroll this machine's UI with an orchestrator
  [run] [--refresh SECS] [--once] [--config-dir DIR]
        the dashboard, reading from the orchestrator this UI joined.
        Enter opens a workspace's agent, s its shell (offered if it has none), in a terminal
        carried over Flight's own connections (no ssh, no direct path to the node), on this
        machine or another. Inside: Ctrl-Space a / s switches between the agent and the shell,
        Ctrl-Space v shows both side by side (inside that: | and - split, t tab, x close,
        n / p tabs, h j k l or o move the keyboard, < > + _ resize, a / s show the agent or
        shell here; the arrangement is remembered per workspace), Ctrl-Space q leaves, and Ctrl-Space Ctrl-Space sends a literal Ctrl-Space.";

pub fn run_ui(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("join") => {
            let parsed = Args::parse(
                &args[1..],
                &["--name", "--bundle-file", "--config-dir"],
                &[],
            )?;
            join_command(&ui_dir(&config_dir(&parsed)?), RoleCode::Ui, &parsed)
        }
        Some("run") => dashboard(&args[1..]),
        _ => dashboard(args),
    }
}

fn dashboard(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &["--refresh", "--config-dir"], &["--once"])?;
    let base = config_dir(&args)?;
    let dir = ui_dir(&base);
    let config = ClientConfig::load(&dir).map_err(|e| {
        format!("this UI has not joined an orchestrator yet ({e}); run `flight ui join`")
    })?;
    let refresh = match args.value("--refresh") {
        Some(s) => Duration::from_secs(s.parse().map_err(|_| format!("bad --refresh {s:?}"))?),
        None => Duration::from_secs(1),
    };
    // Every session is shown in Flight's own terminal, on this machine or another.
    let switcher = Switcher::default();
    let handle = |link: &OrchestratedBackend| {
        let mut backend = link.clone();
        backend.set_switching(switcher.clone());
        backend
    };
    let link = OrchestratedBackend::start(config.clone()).map_err(|e| e.to_string())?;
    if args.switch("--once") {
        return once(handle(&link));
    }
    // A surface is shown in a terminal session over Flight; when it ends the dashboard comes
    // back, with the reason on its status line. The link to the orchestrator is the same one
    // throughout: showing a surface and coming back rebuilds nothing.
    let mut begin = Start::default();
    loop {
        let backend = handle(&link);
        let handoff = backend.handoff();
        let exit = run_with_start(backend, refresh, std::mem::take(&mut begin))
            .map_err(|e| e.to_string())?;
        let Exit::Switched { typed_ahead } = exit else {
            return Ok(());
        };
        match handoff.take() {
            Some(Handoff::Attach(attach)) => {
                // Only returns if the program could not be started: the pane is already selected.
                let why = attach.exec();
                return Err(format!("pane selected, but cannot attach: {why}"));
            }
            Some(Handoff::Terminal { id, shown, binding }) => {
                let outcome = run_session(
                    &link,
                    SessionRequest {
                        id,
                        shown: shown.clone(),
                        binding,
                        typed_ahead,
                    },
                );
                let outcome = if outcome.end == TerminalEnd::Presenting {
                    present(&link, &dir, &shown)
                } else {
                    outcome
                };
                let mut notice = format!("terminal: {}", outcome.end);
                if outcome.undelivered > 0 {
                    notice.push_str(&format!(
                        " ({} typed bytes were not delivered)",
                        outcome.undelivered
                    ));
                }
                // Back on the workspace that was just shown.
                begin = Start {
                    notice: Some(notice),
                    select: Some(shown.workspace),
                    ..Start::default()
                };
            }
            None => return Ok(()),
        }
    }
}

/// Show a workspace's surfaces side by side (or as the user arranged them last time), remember
/// the arrangement they leave, and report as a finished session. A layout that cannot be read or
/// saved is said, and the arrangement is simply not remembered.
fn present(
    link: &OrchestratedBackend,
    dir: &std::path::Path,
    shown: &flight_client::ShownSurface,
) -> flight_client::SessionOutcome {
    let focus = SurfaceId::new(match shown.choice {
        flight_ui::SurfaceChoice::Agent => "agent",
        flight_ui::SurfaceChoice::Shell => "shell",
    });
    let mut store = LayoutStore::open(dir);
    let layout = match &store {
        Ok(store) => starting_layout(Some(store), &shown.workspace, &focus, |s| {
            matches!(s.as_str(), "agent" | "shell")
        }),
        Err(_) => side_by_side(&focus),
    };
    let outcome = run_presentation(link, shown.workspace.clone(), layout);
    let mut said = None;
    match &mut store {
        Ok(store) => {
            if let Err(why) = remember(store, &shown.workspace, &outcome.layout) {
                said = Some(format!("layout not remembered: {why}"));
            }
        }
        Err(why) => said = Some(format!("layout not remembered: {why}")),
    }
    if let Some(said) = said {
        eprintln!("{said}");
    }
    flight_client::SessionOutcome {
        end: outcome.end,
        shown: Some(shown.choice),
        undelivered: outcome.undelivered,
    }
}

/// Wait briefly for the orchestrator's snapshot, then print one frame as text.
fn once(mut backend: OrchestratedBackend) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !backend.connected() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut vm = ViewModel::new();
    vm.apply_snapshot(backend.snapshot(now()));
    if let Some(sel) = vm.selected() {
        vm.apply_preview(Some(backend.preview(&sel)));
    }
    println!("{}", render_to_string(&vm, 110, 32));
    Ok(())
}
