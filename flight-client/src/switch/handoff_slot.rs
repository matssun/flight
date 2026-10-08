// SPDX-License-Identifier: MIT

use crate::switch::Handoff;
use std::sync::{Arc, Mutex};

/// Where the dashboard's worker leaves the attach that must run once the terminal has been
/// handed back. Shared, because the backend lives on the worker thread and the binary reads
/// the slot after the dashboard returns.
#[derive(Debug, Clone, Default)]
pub struct HandoffSlot(Arc<Mutex<Option<Handoff>>>);

impl HandoffSlot {
    pub fn put(&self, handoff: Handoff) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(handoff);
    }

    pub fn take(&self) -> Option<Handoff> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}
