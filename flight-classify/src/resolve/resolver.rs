// SPDX-License-Identifier: MIT

use super::done_machine::advance;
use super::glyph_debounce::debounce_glyph;
use super::{Provenance, ResolveInput, ResolvedState, Tracking};
use crate::{fuse, Evidence};

/// Resolve one pane's state: previous state + current evidence + time -> new state.
///
/// A pane with a hook layer is stateless: the fusion result stands. A hook-less pane
/// (`evidence.hook == None`) additionally runs the glyph debounce and the DONE machine, so
/// it can show Done for "finished while you were elsewhere".
pub fn resolve(input: &ResolveInput<'_>) -> ResolvedState {
    let ev = input.evidence;
    let prev = input.previous.map(|p| p.tracking).unwrap_or_default();

    if ev.hook.is_some() {
        let fused = fuse(ev);
        return ResolvedState {
            state: fused.state,
            provenance: Provenance {
                fused,
                synthesized_done: false,
            },
            tracking: prev,
            observed_at: ev.now,
        };
    }

    let (working, anchor) = debounce_glyph(
        ev.agent,
        input.glyph_seen,
        prev.glyph_anchor,
        ev.now,
        input.idle_secs,
    );
    let fused = fuse(&Evidence {
        working_glyph: working,
        ..ev.clone()
    });
    let (state, tracking) = advance(
        fused.state,
        ev.focused,
        Tracking {
            glyph_anchor: anchor,
            ..prev
        },
    );
    let synthesized_done = state != fused.state;
    ResolvedState {
        state,
        provenance: Provenance {
            fused,
            synthesized_done,
        },
        tracking,
        observed_at: ev.now,
    }
}
