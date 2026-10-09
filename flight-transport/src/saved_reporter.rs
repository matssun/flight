// SPDX-License-Identifier: MIT

use crate::NodeLink;
use flight_node::TmuxServers;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// How often a node re-reads its saved workspaces against what runs and the filesystem. Saved
/// workspaces change rarely and the check touches the disk, so this is much slower than the
/// pane poll; the link sends a delta only when the report actually changed.
pub const SAVED_REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// Keep the link informed of the node's saved workspaces until `stop` flips.
pub async fn run_saved_reporter(
    link: Arc<NodeLink>,
    servers: Arc<TmuxServers>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let source = servers.clone();
        // A panic in one report must not end reporting for good.
        if let Ok(report) = tokio::task::spawn_blocking(move || source.saved_report()).await {
            link.observe_saved(report);
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(interval) => {}
        }
    }
}
