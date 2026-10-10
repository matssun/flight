// SPDX-License-Identifier: MIT

//! `flight` with no arguments: everything on this machine in one command. It starts an
//! orchestrator and a node (as children of this process), enrolls them and the dashboard the
//! first time, and shows the dashboard. When the dashboard quits the children stop; the
//! sessions keep running and are there next time.

use crate::args::{config_dir, default_name, Args};
use crate::join_cmd::join_command;
use crate::roles::{node_dir, orchestrator_dir, ui_dir};
use flight_proto::RoleCode;
use flight_transport::{admin_request, config_path};
use flight_trust::ConnectionConfig;
use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const USAGE: &str = "usage: flight [solo] [--config-dir DIR] [--socket NAME]

  Runs Flight on this machine: starts the orchestrator and the node in the background,
  joins them and the dashboard the first time, and shows the dashboard. Quitting stops
  them again; your sessions keep running. --socket names the private session server
  (default 'flight').";

/// Children of the dashboard process, stopped when it ends.
struct Children(Vec<Child>);

impl Drop for Children {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let parsed = Args::parse(args, &["--config-dir", "--socket", "--listen"], &[])?;
    let base = config_dir(&parsed)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let log = base.join("solo.log");
    let mut children = Children(Vec::new());
    let admin = orchestrator_dir(&base).join("admin.sock");

    if !orchestrator_up(&admin) {
        // Where the orchestrator listens is given on the first run only: the node and the
        // dashboard are enrolled with the address it advertises. (Not in the usage text: it is
        // for running more than one, as the tests do.)
        let mut orchestrator_args = vec!["orchestrator", "run", "--config-dir"];
        let base_str = path(&base);
        orchestrator_args.push(base_str.as_str());
        if let Some(listen) = parsed.value("--listen") {
            orchestrator_args.extend(["--listen", listen]);
        }
        children.0.push(spawn(&exe, &orchestrator_args, &log)?);
        wait_for("the orchestrator", || orchestrator_up(&admin)).map_err(|e| {
            format!(
                "{e}. Is another program using the port (7676 unless --listen)? See {}",
                log.display()
            )
        })?;
    }
    let name = default_name();
    if ConnectionConfig::load(&config_path(&node_dir(&base))).is_err() {
        enroll(&base, RoleCode::Node, &node_dir(&base), &name)?;
    }
    if flight_client::ClientConfig::load(&ui_dir(&base)).is_err() {
        enroll(&base, RoleCode::Ui, &ui_dir(&base), &name)?;
    }
    let base_str = path(&base);
    let mut node_args = vec!["node", "run", "--config-dir", base_str.as_str()];
    if let Some(socket) = parsed.value("--socket") {
        node_args.extend(["--socket", socket]);
    }
    children.0.push(spawn(&exe, &node_args, &log)?);

    crate::ui_cmd::run_ui(&["run".into(), "--config-dir".into(), path(&base)])
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn spawn(exe: &Path, args: &[&str], log: &Path) -> Result<Child, String> {
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", args.first().unwrap_or(&"flight")))
}

fn orchestrator_up(admin: &Path) -> bool {
    tokio::runtime::Runtime::new()
        .ok()
        .is_some_and(|rt| rt.block_on(admin_request(admin, "status")).is_ok())
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        if Instant::now() > deadline {
            return Err(format!("{what} did not start"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Join `role_dir` to the orchestrator under `base`, with a one-time bundle it just made.
fn enroll(base: &Path, role: RoleCode, role_dir: &Path, name: &str) -> Result<(), String> {
    println!("Setting up Flight on this machine ({name})…");
    let admin = orchestrator_dir(base).join("admin.sock");
    let bundle = tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(admin_request(&admin, "enroll"))
        .map_err(|e| e.to_string())?;
    let mut words = vec!["--name".to_owned(), name.to_owned()];
    words.extend(bundle.split_whitespace().map(str::to_owned));
    let parsed = Args::parse(&words, &["--name"], &[])?;
    join_command(role_dir, role, &parsed)
}
