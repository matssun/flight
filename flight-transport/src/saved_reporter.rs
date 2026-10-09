// SPDX-License-Identifier: MIT

use crate::NodeLink;
use flight_node::TmuxServers;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How often a node re-reads its saved workspaces against what runs and the filesystem. Saved
/// workspaces change rarely and the check touches the disk, so this is much slower than the
/// pane poll; the link sends a delta only when the report actually changed.
pub const SAVED_REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// How often the wait for the next report looks for a user's "retry".
const WAKE_EVERY: Duration = Duration::from_millis(200);

/// Keep the link informed of the node's saved workspaces until `stop` flips. A user's retry (or
/// any saved-workspace action, whose effect should show at once) brings the next report
/// forward.
pub async fn run_saved_reporter(
    link: Arc<NodeLink>,
    servers: Arc<TmuxServers>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let seen = servers.retries();
        let source = servers.clone();
        // A panic in one report must not end reporting for good.
        if let Ok(report) = tokio::task::spawn_blocking(move || source.saved_report()).await {
            link.observe_saved(report);
        }
        let due = Instant::now() + interval;
        while Instant::now() < due && servers.retries() == seen {
            tokio::select! {
                _ = stop.changed() => return,
                _ = tokio::time::sleep(WAKE_EVERY) => {}
            }
        }
    }
}
