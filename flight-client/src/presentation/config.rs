// SPDX-License-Identifier: MIT

use super::command::Shown;
use super::surface_names::{surface_choice, surface_id};
use crate::session::{FromRemote, OpenFailure, Reattach};
use flight_present::Style;
use flight_state::SurfaceId;
use std::sync::Arc;
use std::time::Duration;

use crate::screens::Theme;
use flight_ui::SurfaceChoice;

/// What a surface of a layout is, for the host.
pub type Resolve = Arc<dyn Fn(&SurfaceId) -> Option<SurfaceChoice> + Send + Sync>;

/// How a presentation runs.
pub struct PresentationConfig {
    /// Typed bytes waiting for a surface that cannot take them yet; more is not read.
    pub input_limit: usize,
    pub open_timeout: Duration,
    /// How long a new attachment to a surface waits for the old one to finish.
    pub retire_wait: Duration,
    pub lease_period: Duration,
    /// Waits before each re-attachment of a surface whose stream broke; then it stays down.
    pub reattach_delays: Vec<Duration>,
    /// The surfaces a workspace has, in the order a new split takes them.
    pub surfaces: Vec<SurfaceId>,
    /// What each surface is, for the host. `None`: it cannot be attached (shown as unavailable).
    pub resolve: Resolve,
    /// The name shown on a tab.
    pub label: Arc<dyn Fn(&SurfaceId) -> String + Send + Sync>,
    /// The layout rules (minimum tile size).
    pub style: Style,
    pub theme: Theme,
    /// Longest between two repaints while output keeps arriving.
    pub frame: Duration,
    pub say: Arc<dyn Fn(&str) + Send + Sync>,
}

impl PresentationConfig {
    /// A workspace's agent and shell, by their usual names.
    pub fn for_workspace(say: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        let agent = surface_id(SurfaceChoice::Agent);
        let shell = surface_id(SurfaceChoice::Shell);
        Self {
            input_limit: 64 * 1024,
            open_timeout: Duration::from_secs(15),
            retire_wait: Duration::from_secs(3),
            lease_period: crate::terminal::LEASE_PERIOD,
            reattach_delays: Reattach::default_delays(),
            surfaces: vec![agent, shell],
            resolve: Arc::new(surface_choice),
            label: Arc::new(|s| s.to_string()),
            style: Style::default(),
            theme: Theme::default(),
            frame: Duration::from_millis(16),
            say,
        }
    }

    pub(super) fn surface_for(&self, shown: Shown) -> Option<SurfaceId> {
        let wanted = surface_id(match shown {
            Shown::Agent => SurfaceChoice::Agent,
            Shown::Shell => SurfaceChoice::Shell,
        });
        self.surfaces.iter().find(|s| **s == wanted).cloned()
    }
}

/// Words for a tile whose surface could not be attached.
pub(super) fn failure_text(failure: &OpenFailure) -> String {
    match failure {
        OpenFailure::Refused(why) => format!("cannot be shown: {why}"),
        OpenFailure::Unavailable(why) => format!("not reachable: {why}"),
    }
}

/// Words for a tile whose attachment ended.
pub(super) fn remote_text(end: &FromRemote) -> String {
    match end {
        FromRemote::Exit { reason, status } => crate::terminal::TerminalEnd::Exited {
            reason: *reason,
            status: *status,
        }
        .to_string(),
        FromRemote::Lost(why) => format!("connection lost: {why}"),
        FromRemote::Data(_) => String::new(),
    }
}
