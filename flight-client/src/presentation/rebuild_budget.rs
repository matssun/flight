// SPDX-License-Identifier: MIT

/// How many times in a row a tile's screen may be rebuilt from its surface after the emulator
/// failed on what the surface wrote. A surface that sends the same bytes every time it is drawn
/// would otherwise be redrawn, fail and be redrawn for ever.
const MAX_REBUILDS: u8 = 3;

/// A screen that has followed this much without failing is working again: earlier failures no
/// longer count against it.
const HEALTHY_BYTES: usize = 1 << 20;

#[derive(Debug, Default)]
pub(super) struct RebuildBudget {
    rebuilds: u8,
    followed: usize,
}

impl RebuildBudget {
    /// The screen took `bytes` without failing.
    pub(super) fn followed(&mut self, bytes: usize) {
        self.followed = self.followed.saturating_add(bytes);
    }

    /// The screen failed. Whether to rebuild it from the surface (true) or give the tile up.
    pub(super) fn rebuild(&mut self) -> bool {
        if self.followed >= HEALTHY_BYTES {
            self.rebuilds = 0;
        }
        self.followed = 0;
        if self.rebuilds >= MAX_REBUILDS {
            return false;
        }
        self.rebuilds = self.rebuilds.saturating_add(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_in_a_row_run_out_and_a_long_healthy_stretch_resets_them() {
        let mut budget = RebuildBudget::default();
        assert!(budget.rebuild() && budget.rebuild() && budget.rebuild());
        assert!(!budget.rebuild(), "the fourth in a row is not rebuilt");
        budget.followed(HEALTHY_BYTES - 1);
        assert!(!budget.rebuild(), "just short of healthy");
        budget.followed(HEALTHY_BYTES);
        assert!(budget.rebuild(), "healthy again, so a failure starts over");
    }
}
