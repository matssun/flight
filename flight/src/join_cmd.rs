// SPDX-License-Identifier: MIT

use crate::args::{default_name, now, Args};
use flight_proto::RoleCode;
use flight_transport::join;
use flight_trust::EnrollmentBundle;
use std::path::Path;

/// The shared body of `flight node join` and `flight ui join`.
pub fn join_command(role_dir: &Path, role: RoleCode, args: &Args) -> Result<(), String> {
    let text = match args.value("--bundle-file") {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
        None => args.positional.join(" "),
    };
    if text.trim().is_empty() {
        return Err(
            "no enrollment bundle given (pass it as arguments, or --bundle-file PATH)".to_owned(),
        );
    }
    let bundle = EnrollmentBundle::parse(&text).map_err(|e| e.to_string())?;
    let name = args
        .value("--name")
        .map_or_else(default_name, str::to_owned);
    let joined = tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(join(role_dir, &bundle, &name, role, now()))
        .map_err(|e| format!("could not join: {e}"))?;
    println!(
        "joined orchestrator {} as {}",
        joined.orchestrator, joined.node_id
    );
    println!("settings saved under {}", role_dir.display());
    Ok(())
}
