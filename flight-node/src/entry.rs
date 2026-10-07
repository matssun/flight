// SPDX-License-Identifier: MIT

use flight_classify::ResolvedState;
use flight_proto::PaneState;

/// Everything the node holds for one pane. Only `state` is ever replicated; `resolved`
/// carries the Tracking (`was_busy`, `done`, `glyph_anchor`) and `pid` guards id reuse.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) state: PaneState,
    pub(crate) resolved: ResolvedState,
    pub(crate) pid: u32,
}
