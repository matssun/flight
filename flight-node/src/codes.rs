// SPDX-License-Identifier: MIT

use flight_classify::{AgentKind, Reason, ResolvedState, ScrapeVia, Source};
use flight_proto::{AgentKindCode, SourceCode};

pub(crate) fn agent_code(kind: AgentKind) -> AgentKindCode {
    match kind {
        AgentKind::Claude => AgentKindCode::Claude,
        AgentKind::Codex => AgentKindCode::Codex,
        AgentKind::OpenCode => AgentKindCode::OpenCode,
        AgentKind::Pi => AgentKindCode::Pi,
        AgentKind::Other => AgentKindCode::Other,
    }
}

/// The coarse provenance the wire carries; rule-level detail stays in the node.
pub(crate) fn source_code(r: &ResolvedState) -> SourceCode {
    if r.provenance.synthesized_done {
        return SourceCode::SynthesizedDone;
    }
    let f = &r.provenance.fused;
    match f.source {
        Source::Hook => SourceCode::Hook,
        Source::Event => SourceCode::Event,
        Source::Glyph => SourceCode::Glyph,
        Source::Scrape => match f.candidates.scrape_via {
            Some(ScrapeVia::Title) => SourceCode::Title,
            _ => SourceCode::Scrape,
        },
        Source::Default if f.reason == Reason::WorkingTimedOut => SourceCode::Timeout,
        Source::Default => SourceCode::Default,
    }
}
