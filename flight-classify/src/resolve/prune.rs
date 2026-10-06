// SPDX-License-Identifier: MIT

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Drop tracking for panes no longer seen (agent exited, pane closed), so a long session
/// cannot grow the map without bound. Ported from Fleet's `pruneDoneTracking`.
pub fn prune_tracking<K: Hash + Eq, V>(map: &mut HashMap<K, V>, live: &HashSet<K>) {
    map.retain(|k, _| live.contains(k));
}
