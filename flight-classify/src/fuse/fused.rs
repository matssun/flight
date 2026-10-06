// SPDX-License-Identifier: MIT

use crate::{Classification, RuleId};
use flight_state::AgentState;

/// Which evidence decided the final state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The screen or title classification.
    Scrape,
    Event,
    Hook,
    /// The process-scan working glyph.
    Glyph,
    /// Nothing decided it: a working state timed out to Idle.
    Default,
}

/// Which scrape evidence filled the scrape slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapeVia {
    Title,
    Screen,
}

/// Why the winner won. A closed set, so provenance can be matched on, not parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// An on-screen permission or question prompt, trusted absolutely.
    PromptOnScreen,
    /// A live working indicator on screen or title.
    LiveWorkingIndicator,
    /// A bare prompt on screen cleared a stale permit/question.
    BarePromptClearedStalePrompt,
    /// Working with no activity for too long.
    WorkingTimedOut,
    /// Derived from the latest event.
    LatestEvent,
    /// Derived from the hook status file.
    HookStatus,
    /// Activity glyph in the process scan.
    ActivityGlyph,
    /// No hook, event, glyph or screen evidence at all.
    NoEvidence,
}

/// What each layer said, whether or not it won.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidates {
    /// `None` when there is no hook layer.
    pub hook: Option<AgentState>,
    pub event: Option<AgentState>,
    /// The scrape slot: the (refined) title if it fired, else the live screen.
    pub scrape: Option<Classification>,
    pub scrape_via: Option<ScrapeVia>,
}

/// The final state with full provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusedClassification {
    pub state: AgentState,
    pub source: Source,
    pub reason: Reason,
    pub candidates: Candidates,
    /// The hook/event layer decayed from Busy to Idle for inactivity. True even if the
    /// screen then overrode the result.
    pub working_timeout_fired: bool,
}

impl FusedClassification {
    /// The rule that decided the state, when the scrape slot decided it.
    pub fn rule_id(&self) -> Option<&RuleId> {
        match self.source {
            Source::Scrape => self.candidates.scrape.as_ref().map(|c| &c.rule_id),
            _ => None,
        }
    }
}
