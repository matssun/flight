// SPDX-License-Identifier: MIT

/// What recovery may do. Everything that starts a process is off by default: looking at what
/// is saved and what is running, and reconnecting to what already runs, need no permission;
/// starting something does. There is no switch for touching the filesystem because there is no
/// such action (see `Action`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryPolicy {
    /// Start surfaces that are saved but not running (a replacement process).
    pub start_missing: bool,
    /// Start surfaces from definitions that were imported rather than made here.
    pub start_imported: bool,
    /// Start an agent that was saved as running without permission prompts.
    pub start_skip_permissions: bool,
    /// Providers whose sessions can be resumed rather than replaced. Empty today: no provider
    /// is wired for resumption, so an agent is only ever reconnected or replaced.
    pub resumable_providers: Vec<String>,
}
