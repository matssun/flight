// SPDX-License-Identifier: MIT

use crate::shared::now;
use crate::NodeLink;
use flight_node::TmuxServers;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Poll the node's tmux servers on a blocking thread (never on the runtime, never under the
/// session lock) and feed what was seen to the link, until `stop` flips.
pub async fn run_observer(
    link: Arc<NodeLink>,
    servers: Arc<TmuxServers>,
    interval: Duration,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let polled = servers.clone();
        let rounds = tokio::task::spawn_blocking(move || polled.observe(now())).await;
        if let Ok(rounds) = rounds {
            link.observe(rounds);
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(interval) => {}
        }
    }
}
