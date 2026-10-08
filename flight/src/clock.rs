// SPDX-License-Identifier: MIT

/// One timestamped operator log line on stdout. A closed or full stdout (`... | head`, a
/// supervisor that stopped reading) must not take the service down: `println!` would panic
/// inside whatever task logged, holding the orchestrator's lock.
pub fn log_line(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{} {line}", utc_clock());
}

/// `HH:MM:SS` UTC, for log lines (no time-zone database needed).
pub fn utc_clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}
