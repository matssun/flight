// SPDX-License-Identifier: MIT

use super::{EventObservation, HookObservation};
use crate::{AgentKind, Classification};

/// Everything known about one pane at `now`, already classified by the layer that saw it.
/// The fuser never observes; it only weighs.
#[derive(Debug, Clone)]
pub struct Evidence {
    pub agent: AgentKind,
    pub screen: Option<Classification>,
    /// When the screen was captured. `None` means it is live. A screen older than the
    /// newest hook or event is discarded: the screen has changed since (a prompt was
    /// answered, a turn ended).
    pub screen_captured_at: Option<u64>,
    pub title: Option<Classification>,
    /// `None` means there is no hook layer for this pane (a hook-less discovered agent).
    pub hook: Option<HookObservation>,
    pub event: Option<EventObservation>,
    /// The user is looking at this pane right now. Used only by the temporal resolver.
    pub focused: bool,
    /// A working glyph in the process scan. Counts as activity only when there is no event.
    pub working_glyph: bool,
    /// Seconds since the epoch.
    pub now: u64,
}

impl Evidence {
    /// Evidence with nothing observed.
    pub fn empty(agent: AgentKind, now: u64) -> Self {
        Self {
            agent,
            screen: None,
            screen_captured_at: None,
            title: None,
            hook: None,
            event: None,
            focused: false,
            working_glyph: false,
            now,
        }
    }
}
