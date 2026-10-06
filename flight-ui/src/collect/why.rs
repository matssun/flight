// SPDX-License-Identifier: MIT

use flight_classify::{Reason, ResolvedState, Source};

/// A short human reason for a resolved state, for the dashboard.
pub(super) fn why(r: &ResolvedState) -> String {
    if r.provenance.synthesized_done {
        return "finished while away".to_owned();
    }
    let f = &r.provenance.fused;
    match (f.source, f.rule_id()) {
        (Source::Scrape, Some(rule)) => rule.to_string(),
        (_, _) => match f.reason {
            Reason::ActivityGlyph => "working glyph".to_owned(),
            Reason::NoEvidence => "no activity".to_owned(),
            Reason::WorkingTimedOut => "working timed out".to_owned(),
            other => format!("{other:?}").to_lowercase(),
        },
    }
}
